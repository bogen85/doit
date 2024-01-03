// code: language=Rust insertSpaces=true tabSize=2

use getopts::Options;
use std::process::{exit, Command};
use std::{env, fs::File, io::Read};
use toml_edit::{Array, Document, Table};

fn read_doit_file() -> Document {
  let mut contents = String::new();
  File::open("doit.toml")
    .expect("Unable to open file")
    .read_to_string(&mut contents)
    .expect("Unable to read file");
  contents.parse::<Document>().expect("Unable to parse TOML")
}

const ASCII_SUB: &str = "\x1A";

fn render_template(table: &Table, template: &str) -> String {
  let mut render = template.to_string().replace("%%", &ASCII_SUB);
  for (key, value) in table.iter() {
    if let Some(val) = value.as_str() {
      render = render.replace(&format!("%{}%", key), val);
    }
  }
  render.replace(&ASCII_SUB, "%%")
}

fn process_pre_post_cmd(which: &str, cmd_name: &str, table: &Table) {
  let sub_args = table[which]
    .as_array()
    .expect(&format!("{} is not an array", which));

  for (index, args_in) in sub_args.iter().enumerate() {
    println!("Running command {}:{}:{}", cmd_name, which, index + 1);

    let args = {
      let vec_in = args_in
        .as_array()
        .expect(&format!("{}[{}] is not an array", which, index));
      let mut vec: Vec<String> = Vec::new();
      for arg in vec_in {
        vec.push(render_template(
          table,
          &arg
            .as_str()
            .expect(&format!("Invalid argument for {}:[{}]", which, index))
            .to_string(),
        ));
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
      panic!("Exit status: {}", rc);
    }
  }
}

fn process_cmd(cmd_name: &str, table: &Table, additional_args: &[String]) {
  if table.contains_key("pre") {
    process_pre_post_cmd("pre", cmd_name, &table);
  }

  println!("Running command {}", cmd_name);

  let command = table["command"]
    .as_str()
    .expect(&format!("{}: missing command", cmd_name));

  let args = {
    let mut args = vec![command.to_string()];

    let toml_args: Array = if table.contains_key("args") {
      table["args"].as_array().expect("Array!").clone()
    } else {
      Array::default()
    };
    for arg in toml_args {
      args.push(render_template(
        table,
        arg.as_str().expect("Invalid argument"),
      ));
    }
    args.extend_from_slice(additional_args);
    args
  };

  let exit_status = {
    let cmd = &args[0];
    let argv = &args[1..];
    let mut child = Command::new(cmd)
      .args(argv)
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
    panic!("Exit status: {}", rc);
  }
}

fn primary(cmd_name: &str, additional_args: &[String]) -> bool {
  let doc = read_doit_file();
  return if doc.contains_key(&cmd_name) {
    if let Some(table) = doc[&cmd_name].as_table() {
      process_cmd(&cmd_name, &table, &additional_args);
    }
    true
  } else {
    false
  };
}

fn list_cmds() {
  for (i, (cmd, _)) in read_doit_file().as_table().iter().enumerate() {
    println!("{}: {}", i + 1, cmd);
  }
}

fn print_usage(program: &str, opts: Options) {
  let brief = format!("Usage: {} <command> [args...]", program);
  print!("{}", opts.usage(&brief));
}

fn print_about(program: &str) {
  println!("program: {}", program);
  println!("version: {}", env!("CARGO_PKG_VERSION"));
  println!("author: {}", env!("CARGO_PKG_AUTHORS"));
  println!("about: {}", env!("CARGO_PKG_DESCRIPTION"));
}

fn show_details(cmd_name: &str) -> bool {
  let doc = read_doit_file();

  if !doc.contains_key(cmd_name) {
    return false;
  }

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
        .map(|arg| render_template(table, &arg.to_string()))
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
  return true;
}

fn main() {
  let args: Vec<String> = env::args().collect();
  let program = args[0].clone();

  let opts = {
    let mut opts = Options::new();
    opts.optflag("", "help", "print this help menu");
    opts.optflag("", "cmds", "list all available commands");
    opts.optflag("", "about", "about this program");
    opts.optopt("", "show", "show details for command", "command");
    opts
  };

  let matches = match opts.parse(&args[1..]) {
    Ok(m) => m,
    Err(f) => {
      panic!("{}", f.to_string())
    }
  };

  if matches.opt_present("help") {
    return print_usage(&program, opts);
  }

  if matches.opt_present("about") {
    return print_about(&program);
  }

  if let Some(cmd_name) = matches.opt_str("show") {
    if !show_details(&cmd_name) {
      println!("{} not found", cmd_name);
      print_usage(&program, opts);
      exit(1);
    }
    return;
  }

  if matches.opt_present("cmds") {
    return list_cmds();
  }

  let cmd_name = if !matches.free.is_empty() {
    matches.free[0].clone()
  } else {
    print_usage(&program, opts);
    exit(1);
  };

  let additional_args = if matches.free.len() > 1 {
    matches.free[1..].to_vec()
  } else {
    vec![]
  };
  if !primary(&cmd_name, &additional_args) {
    println!("{} not found", cmd_name);
    print_usage(&program, opts);
    exit(1);
  }
}
