use std::{cmp::Reverse, time::Duration};

use humantime::format_duration;
use serde::Serialize;

use crate::{
    envs,
    input::layout::{FloatingPaneLayout, Layout, Run, RunPluginOrAlias, TiledPaneLayout},
    sessions::{get_resurrectable_sessions, get_sessions, resurrection_layout},
};

#[derive(Serialize, Debug, Clone)]
struct Session {
    name: String,
    tabs: Vec<Tab>,
    timestamp: Duration,
    is_current: bool,
    is_active: bool,
}

#[derive(Serialize, Debug, Clone)]
struct Tab {
    name: Option<String>,
    commands: Vec<MyRun>,
}

#[derive(Debug, Clone)]
struct MyRun(Run);

#[derive(Serialize, Debug, Clone)]
struct MyCommand {
    command: String,
    cwd: String,
}

pub fn print_session_by_name(session_name: &str, no_formatting: bool) {
    let sessions = collect_sessions();
    if let Some(session) = sessions.iter().find(|s| s.name == session_name) {
        if no_formatting {
            print_unformatted_session(session);
        } else {
            print_session(session);
        }
    } else {
        println!("No session found with the name {session_name}");
    }
}

pub fn list_sessions_long(json: bool, no_formatting: bool, reverse: bool) {
    let mut sessions = collect_sessions();

    if reverse {
        sessions.sort_unstable_by_key(|session| session.timestamp);
    } else {
        sessions.sort_unstable_by_key(|session| Reverse(session.timestamp));
    }

    if json {
        print_sessions_json(sessions);
    } else if no_formatting {
        for session in &sessions {
            print_unformatted_session(session);
        }
    } else {
        for session in sessions {
            print_session(&session);
        }
    }
}

fn print_sessions_json(sessions: Vec<Session>) {
    println!(
        "{}",
        serde_json::to_string(&sessions).expect("Should always serialize correctly")
    );
}

fn print_session(session: &Session) {
    let unnamed_tab_str = String::from("<Unnamed Tab>");
    let formatted_session_name = format!("\u{1b}[32;1m{}\u{1b}[m", session.name);
    let timestamp = format!(
        "[Created \u{1b}[35;1m{}\u{1b}[m ago]",
        format_duration(session.timestamp)
    );
    let current_text = if session.is_current { " (current)" } else { "" };
    println!("{} {}{}", formatted_session_name, timestamp, current_text);
    if session.tabs.is_empty() {
        // Indent by 2 spaces
        println!("  No running commands");
    } else {
        for tab in &session.tabs {
            let tab_name: &str = tab.name.as_ref().unwrap_or(&unnamed_tab_str);
            let formatted_tab_name = format!("\u{1b}[36;1m{}\u{1b}[m", tab_name);
            println!("{}:", formatted_tab_name);

            // Indent by 2 spaces
            for command in &tab.commands {
                println!(" {}", display_run(&command.0, true));
            }
        }
    }
    // Empty line between sessions
    println!();
}

fn print_unformatted_session(session: &Session) {
    let unnamed_tab_str = String::from("<Unnamed Tab>");
    let current_text = if session.is_current { " (current)" } else { "" };
    let timestamp = format!("Created {} ago", format_duration(session.timestamp));
    println!("{} {}{}", session.name, timestamp, current_text);

    if session.tabs.is_empty() {
        println!("No running commands");
    } else {
        for tab in &session.tabs {
            let tab_name = tab.name.as_ref().unwrap_or(&unnamed_tab_str);
            println!("{tab_name}:");
            for command in &tab.commands {
                println!("{}", display_run(&command.0, false));
            }
        }
    }
}

fn collect_sessions() -> Vec<Session> {
    let curr_session = envs::get_session_name().unwrap_or_else(|_| "".into());
    let active_session_names: Vec<String> = get_sessions()
        .unwrap()
        .into_iter()
        .map(|(name, _timestamp)| name)
        .collect();

    get_resurrectable_sessions()
        .into_iter()
        .map(|(name, timestamp)| {
            let is_active = active_session_names.contains(&name);
            let is_current = name == curr_session;
            let layout = resurrection_layout(&name).unwrap();

            Session::new(name, timestamp, layout, is_current, is_active)
        })
        .collect()
}

impl Session {
    fn new(
        name: String,
        timestamp: Duration,
        layout: Option<Layout>,
        is_current: bool,
        is_active: bool,
    ) -> Self {
        let tabs = if let Some(layout) = layout {
            layout.tabs
        } else {
            Vec::new()
        };
        let tabs: Vec<Tab> = tabs
            .into_iter()
            .map(|(maybe_name, tile, floating_panes)| {
                Tab::new(maybe_name, tile, floating_panes.into_iter())
            })
            .collect();
        Self {
            name,
            tabs,
            timestamp,
            is_current,
            is_active,
        }
    }
}

impl Tab {
    fn new(
        name: Option<String>,
        tile: TiledPaneLayout,
        floating_panes: impl Iterator<Item = FloatingPaneLayout>,
    ) -> Self {
        let mut tile_commands = Vec::new();
        collect_commands_recursive(tile, &mut tile_commands);
        let floating_commands = floating_panes.filter_map(|float| float.run);
        let commands = tile_commands
            .into_iter()
            .chain(floating_commands)
            .map(MyRun)
            .collect();

        Self { name, commands }
    }
}

impl Serialize for MyRun {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match &self.0 {
            // For commands, include the CWD
            Run::Command(run_command) => {
                let command = format!(
                    "{} {}",
                    run_command.command.to_string_lossy(),
                    run_command.args.join(" ")
                );

                let cwd = match &run_command.cwd {
                    Some(cwd) => cwd.to_string_lossy().to_string(),
                    None => String::new(),
                };

                let my_command = MyCommand { command, cwd };
                my_command.serialize(serializer)
            },
            // For all other types of Run, display as normal
            // TODO: Custom serializers for each type
            // e.g. {
            //  cwd: "foo/bar/baz",
            //  type: "cwd"
            // }
            other => serializer.serialize_str(&display_run(other, false)),
        }
    }
}

fn display_run(run: &Run, should_format: bool) -> String {
    let format_title = if should_format {
        |title| format!("\u{1b}[35;1m{}\u{1b}[m", title)
    } else {
        |title| String::from(title)
    };

    match run {
        Run::Command(run_command) => {
            format!(
                "{} {} {}",
                format_title("Running:"),
                run_command.command.to_string_lossy(),
                run_command.args.join(" ")
            )
        },
        Run::EditFile(path_buf, _, _) => {
            format!("{} {}", format_title("File:"), path_buf.to_string_lossy())
        },
        Run::Cwd(path_buf) => {
            format!("{} {}", format_title("CWD:"), path_buf.to_string_lossy())
        },
        Run::Plugin(plugin) => format!(
            "{} {}",
            format_title("Plugin:"),
            display_plugin_or_alias(plugin)
        ),
    }
}

fn display_plugin_or_alias(plugin_or_alias: &RunPluginOrAlias) -> String {
    match plugin_or_alias {
        RunPluginOrAlias::RunPlugin(run_plugin) => run_plugin.location.to_string(),
        RunPluginOrAlias::Alias(plugin_alias) => plugin_alias.name.to_string(),
    }
}

fn collect_commands_recursive(tile: TiledPaneLayout, buf: &mut Vec<Run>) {
    for child in tile.children {
        if let Some(run) = &child.run {
            buf.push(run.clone());
        }
        collect_commands_recursive(child, buf);
    }
}
