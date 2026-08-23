mod cli;
mod inventory;
mod openssh;
mod paths;
mod store;

use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::cli::{Cli, Command, ConfigCommand, EditArgs, TagCommand};
use crate::inventory::{Host, Inventory};
use crate::openssh::{check_config, resolve, test_connection};
use crate::paths::AppPaths;
use crate::store::Store;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = AppPaths::discover()?;
    let store = Store::new(paths.clone());

    match cli.command {
        None => {
            let inventory = store.load()?;
            print_hosts(&inventory.hosts.iter().collect::<Vec<_>>());
        }
        Some(Command::Show { name }) => {
            let inventory = store.load()?;
            print_host(inventory.get(&name)?);
        }
        Some(Command::Find { text }) => {
            let inventory = store.load()?;
            print_hosts(&inventory.find(&text));
        }
        Some(Command::Resolve { name }) => {
            store.load()?.get(&name)?;
            resolve(&paths, &name)?;
        }
        Some(Command::Test { name, timeout }) => {
            store.load()?.get(&name)?;
            test_connection(&paths, &name, timeout)?;
        }
        Some(Command::Add(args)) => {
            let name = args.name.clone();
            store.mutate(|inventory| {
                inventory.add(Host {
                    name: args.name,
                    hostname: args.hostname,
                    user: args.user,
                    port: args.port,
                    identity_file: args.identity,
                    tags: args.tags,
                })
            })?;
            println!("Added {name}.");
        }
        Some(Command::Edit(args)) => {
            let name = args.name.clone();
            store.mutate(|inventory| edit_host(inventory, args))?;
            println!("Updated {name}.");
        }
        Some(Command::Clone { source, name }) => {
            let destination = name.clone();
            store.mutate(|inventory| inventory.clone_host(&source, name))?;
            println!("Cloned {source} as {destination}.");
        }
        Some(Command::Rename { old, new }) => {
            let renamed = new.clone();
            store.mutate(|inventory| inventory.rename(&old, new))?;
            println!("Renamed {old} to {renamed}.");
        }
        Some(Command::Rm { name }) => {
            store.mutate(|inventory| inventory.remove(&name))?;
            println!("Removed {name}.");
        }
        Some(Command::Tag { command }) => match command {
            TagCommand::Add { name, tags } => {
                store.mutate(|inventory| inventory.add_tags(&name, tags))?;
                println!("Updated tags for {name}.");
            }
            TagCommand::Rm { name, tags } => {
                store.mutate(|inventory| inventory.remove_tags(&name, &tags))?;
                println!("Updated tags for {name}.");
            }
        },
        Some(Command::Config { command }) => match command {
            ConfigCommand::Sync => {
                store.regenerate()?;
                println!("Generated {}.", paths.generated_config.display());
                if !paths.user_config_has_include()? {
                    println!(
                        "To activate it, add this line to {}:\n{}",
                        paths.user_config.display(),
                        paths.include_directive()?
                    );
                }
            }
            ConfigCommand::Check => {
                let report = check_config(&store, &paths);
                report.print();
                if !report.is_ok() {
                    bail!("configuration check failed");
                }
            }
        },
    }

    Ok(())
}

fn edit_host(inventory: &mut Inventory, args: EditArgs) -> Result<()> {
    if !args.has_changes() {
        bail!("no changes specified");
    }

    let host = inventory.get_mut(&args.name)?;
    if let Some(hostname) = args.hostname {
        host.hostname = hostname;
    }
    if let Some(user) = args.user {
        host.user = Some(user);
    }
    if args.clear_user {
        host.user = None;
    }
    if let Some(port) = args.port {
        host.port = Some(port);
    }
    if args.clear_port {
        host.port = None;
    }
    if let Some(identity) = args.identity {
        host.identity_file = Some(identity);
    }
    if args.clear_identity {
        host.identity_file = None;
    }
    Ok(())
}

fn print_host(host: &Host) {
    println!("Name:      {}", host.name);
    println!("HostName:  {}", host.hostname);
    println!("User:      {}", host.user.as_deref().unwrap_or("-"));
    println!(
        "Port:      {}",
        host.port
            .map_or_else(|| "-".into(), |port| port.to_string())
    );
    println!(
        "Identity:  {}",
        host.identity_file.as_deref().unwrap_or("-")
    );
    println!(
        "Tags:      {}",
        if host.tags.is_empty() {
            "-".into()
        } else {
            host.tags.join(", ")
        }
    );
}

fn print_hosts(hosts: &[&Host]) {
    if hosts.is_empty() {
        println!("No destinations.");
        return;
    }

    let rows: Vec<_> = hosts
        .iter()
        .map(|host| (host.name.as_str(), host.destination(), host.tags.join(",")))
        .collect();
    let name_width = rows.iter().map(|row| row.0.len()).max().unwrap_or(4).max(4);
    let destination_width = rows
        .iter()
        .map(|row| row.1.len())
        .max()
        .unwrap_or(11)
        .max(11);

    println!(
        "{:<name_width$}  {:<destination_width$}  TAGS",
        "NAME", "DESTINATION"
    );
    for (name, destination, tags) in rows {
        println!("{name:<name_width$}  {destination:<destination_width$}  {tags}");
    }
}

impl Store {
    fn mutate<F>(&self, operation: F) -> Result<()>
    where
        F: FnOnce(&mut Inventory) -> Result<()>,
    {
        let _lock = self.lock()?;
        let mut inventory = self.load()?;
        operation(&mut inventory)?;
        inventory.normalize();
        inventory.validate()?;
        self.save_and_sync(&inventory)
            .context("saving the inventory and generated config")
    }
}
