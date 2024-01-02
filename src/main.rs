// code: language=Rust insertSpaces=true tabSize=2

use std::env;
use std::fs::File;
use std::io::Read;
use std::process::exit;
use std::process::Command;
use toml_edit::Document;
use toml_edit::Table;

static SUB_COMMAND: &str = ".sub_command";

fn print_help() {
  println!("Usage: doit <command> [args...]");
  println!("       doit --help <command>");
}

fn read_doit_file() -> String {
  let mut contents = String::new();
  File::open("doit.toml")
    .expect("Unable to open file")
    .read_to_string(&mut contents)
    .expect("Unable to read file");
  contents
}

fn print_alias_help(cmd_name: &str) {
  let contents = read_doit_file();

  let doc = contents.parse::<Document>().expect("Unable to parse TOML");

  if let Some(table) = doc[cmd_name].as_table() {
    let command = table["command"].as_str().expect("Missing command");
    let description = if table.contains_key("description") {
      table["description"]
        .as_str()
        .unwrap_or("No description provided")
    } else {
      "No description provided"
    };
    let args = if table.contains_key("description") {
      let toml_args = table["args"].as_array();
      toml_args
        .iter()
        .map(|arg| arg.to_string())
        .collect::<Vec<_>>()
        .join(" ")
    } else {
      String::new()
    };

    println!("Alias: {}", cmd_name);
    println!("Command: {}", command);
    println!("Arguments: {}", args);
    println!("Description: {}", description);
  } else {
    println!("Alias not found");
  }
}

fn process_cmd(cmd_name: &str, table: &Table, additional_args: &[String]) {
  if table.contains_key("pre") {
    println!("Runing {}.pre", cmd_name);
    if let Some(sub_table) = table["pre"].as_table() {
      process_cmd(cmd_name, &sub_table, &[]);
    }
  }

  if cmd_name != SUB_COMMAND {
    println!("Runing command {}", cmd_name);
  }

  let command = table["command"].as_str().expect("Missing command");
  let mut args = vec![command.to_string()];

  let toml_args: toml_edit::Array = if table.contains_key("args") {
    table["args"].as_array().expect("Array!").clone()
  } else {
    toml_edit::Array::default()
  };
  for arg in toml_args {
    args.push(arg.as_str().expect("Invalid argument").to_string());
  }
  args.extend_from_slice(additional_args);

  let exit_status = {
    let mut child = Command::new(&args[0])
      .args(&args[1..])
      .spawn()
      .expect("Failed to execute command");
    child.wait()
  };
  let rc = exit_status.expect("RC").code().unwrap_or(1);

  if rc == 0 {
    if table.contains_key("post") {
      println!("Runing {}.post", cmd_name);
      if let Some(sub_table) = table["post"].as_table() {
        process_cmd(SUB_COMMAND, &sub_table, &[]);
      }
    }
  } else {
    println!("Exit status: {}", rc);
    exit(rc);
  }
}

fn main() {
  let args: Vec<String> = env::args().collect();
  if args.len() < 2 || args[1] == "--help" {
    if args.len() > 2 {
      print_alias_help(&args[2]);
    } else {
      print_help();
    }
    return;
  }

  let cmd_name = &args[1];
  let additional_args = if args.len() > 2 { &args[2..] } else { &[] };

  let contents = read_doit_file();

  let doc = contents.parse::<Document>().expect("Unable to parse TOML");
  if doc.contains_key(cmd_name) {
    if let Some(table) = doc[cmd_name].as_table() {
      process_cmd(cmd_name, &table, additional_args);
    }
  } else {
    println!("Alias not found");
    print_help();
    exit(1)
  }
}
