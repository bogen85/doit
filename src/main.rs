// code: language=Rust insertSpaces=true tabSize=2

use std::env;
use std::fs::File;
use std::io::Read;
use std::process::exit;
use std::process::Command;
use toml_edit::{Array, Document, Table};

fn print_help() {
  println!("Usage: doit <command> [args...]");
  println!("       doit --help <command>");
  println!("       doit --cmds");
}

fn read_doit_file() -> Document {
  let mut contents = String::new();
  File::open("doit.toml")
    .expect("Unable to open file")
    .read_to_string(&mut contents)
    .expect("Unable to read file");
  contents.parse::<Document>().expect("Unable to parse TOML")
}

fn print_alias_help(cmd_name: &str) {
  let doc = read_doit_file();

  if let Some(table) = doc[cmd_name].as_table() {
    let command = table["command"].as_str().expect("Missing command");
    let description = if table.contains_key("description") {
      table["description"]
        .as_str()
        .unwrap_or("No description provided")
    } else {
      "No description provided"
    };
    let args = if table.contains_key("args") {
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
    println!("Arguments:{}", args);
    println!("Description: {}", description);
  } else {
    println!("Alias not found");
  }
}

fn process_pre_post_cmd(which: &str, cmd_name: &str, table: &Table) {
  let sub_args = table[which]
    .as_array()
    .expect(&format!("{} is not an array", which));

  for (index, args_in) in sub_args.iter().enumerate() {
    println!("Runing command {}:{}:{}", cmd_name, which, index + 1);

    let args = {
      let vec_in = args_in
        .as_array()
        .expect(&format!("{}[{}] is not an array", which, index));
      let mut vec: Vec<String> = Vec::new();
      for arg in vec_in {
        vec.push(
          arg
            .as_str()
            .expect(&format!("Invalid argument for {}:[{}]", which, index))
            .to_string(),
        );
      }
      vec
    };

    let cmd = args
      .get(0)
      .expect(&format!("{}[{}] array is empty", which, index));
    let cmd_args = if args.len() > 1 { &args[1..] } else { &[] };

    let exit_status = {
      let mut child = Command::new(&cmd).args(&*cmd_args).spawn().expect(&format!(
        "Failed to execute sub command: {}:{:?}",
        cmd, cmd_args
      ));
      child.wait()
    };
    let rc = exit_status.expect("RC").code().unwrap_or(1);
    if rc != 0 {
      println!("Exit status: {}", rc);
      exit(rc);
    }
  }
}

fn process_cmd(cmd_name: &str, table: &Table, additional_args: &[String]) {
  if table.contains_key("pre") {
    process_pre_post_cmd("pre", cmd_name, &table);
  }

  println!("Runing command {}", cmd_name);

  let command = table["command"]
    .as_str()
    .expect(&format!("{}: missing command", cmd_name));
  let mut args = vec![command.to_string()];

  let toml_args: Array = if table.contains_key("args") {
    table["args"].as_array().expect("Array!").clone()
  } else {
    Array::default()
  };
  for arg in toml_args {
    args.push(arg.as_str().expect("Invalid argument").to_string());
  }
  args.extend_from_slice(additional_args);

  let exit_status = {
    let mut child = Command::new(&args[0])
      .args(&args[1..])
      .spawn()
      .expect(&format!("Failed to execute command: {:?}", args));
    child.wait()
  };
  let rc = exit_status.expect("RC").code().unwrap_or(1);

  if rc == 0 {
    if table.contains_key("post") {
      process_pre_post_cmd("post", cmd_name, &table);
    }
  } else {
    println!("Exit status: {}", rc);
    exit(rc);
  }
}

fn list_cmds() {
  for (i, (cmd, _)) in read_doit_file().as_table().iter().enumerate() {
    println!("{}: {}", i + 1, cmd);
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
  if args.len() < 2 || args[1] == "--cmds" {
    list_cmds();
    return;
  }

  let cmd_name = &args[1];
  let additional_args = if args.len() > 2 { &args[2..] } else { &[] };

  let doc = read_doit_file();
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
