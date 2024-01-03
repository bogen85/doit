// code: language=Rust insertSpaces=true tabSize=2
use dirs;
use getopts::Options;
use once_cell::sync::Lazy;
use regex::Regex;
use std::process::{exit, Command};
use std::{env, fs::File, io::Read};
use toml_edit::{Array, Document, Table};

const ASCII_SUB1: &str = "\x1A\x01";
const ASCII_SUB2: &str = "\x1A\x02";

static HOME: Lazy<String> =
  Lazy::new(|| dirs::home_dir().expect("home_dir undefined").to_str().expect("String").to_string());

static ENV0_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"!\{env:(.*?):(.*?)\}").unwrap());
static ENV1_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"!\{env:(.*?)\}").unwrap());
static VAR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"!\{(.*?)\}").unwrap());

static EMPTY_STRING: Lazy<String> = Lazy::new(|| String::default());

const DOIT_FILE: &str = "doit.toml";

fn read_doit_file() -> Document {
  let mut contents = String::new();
  File::open(DOIT_FILE).expect("Unable to open file").read_to_string(&mut contents).expect("Unable to read file");
  contents.parse::<Document>().expect("Unable to parse TOML")
}

fn render_template(table: &Table, template: &str) -> String {
  let x0 = template.to_string().replace("~~", &ASCII_SUB1).replace("!!", &ASCII_SUB2);

  let x1 = ENV0_RE
    .replace_all(&x0, |caps: &regex::Captures| {
      let evar = &caps[1];
      match env::var(&evar) {
        | Ok(value) => value,
        | Err(_) => caps[2].to_string(),
      }
    })
    .to_string();

  let x2 = ENV1_RE
    .replace_all(&x1, |caps: &regex::Captures| {
      let evar = &caps[1];
      match env::var(&evar) {
        | Ok(value) => value,
        | Err(_) => panic!("(Unknown ENV variable: {})", evar),
      }
    })
    .to_string();

  VAR_RE
    .replace_all(&x2, |caps: &regex::Captures| {
      let key = &caps[1];

      match table.get(key) {
        | Some(value) => format!("{}", value.as_str().expect("String")),
        | None => {
          panic!("(Unknown table key: {})", key)
        }
      }
    })
    .to_string()
    .replace("~", &HOME)
    .replace(&ASCII_SUB1, "~")
    .replace(&ASCII_SUB2, "!")
}

fn run_cmd(args: Vec<String>) {
  let exit_status = {
    let mut child =
      Command::new(&args[0]).args(&args[1..]).spawn().expect(&format!("Failed to execute command: {:?}", args));
    child.wait()
  };
  let rc = exit_status.expect("RC").code().unwrap_or(1);

  if rc != 0 {
    let err = format!("exit status: {}", rc);
    eprintln!("{:?}\nfailed with {}", args, err);
    panic!("{}", err);
  }
}

fn process_args(vec_in: &Array, which: &str, table: &Table, index: usize, additional_args: &[String]) -> Vec<String> {
  if vec_in.len() < 1 {
    panic!("{}[{}] arg vector is empty", which, index);
  }
  let mut vec: Vec<String> = Vec::new();
  for arg in vec_in {
    vec.push(render_template(
      table,
      &arg.as_str().expect(&format!("Invalid argument for {}:[{}]", which, index)).to_string(),
    ));
  }
  vec.extend_from_slice(additional_args);
  vec
}

fn process_pre_post_cmd(which: &str, cmd_name: &str, table: &Table) {
  let sub_args = table[which].as_array().expect(&format!("{} is not an array", which));

  for (index, args_in) in sub_args.iter().enumerate() {
    println!("Running command {}:{}:{}", cmd_name, which, index + 1);
    run_cmd(process_args(
      args_in.as_array().expect(&format!("{}[{}] is not an array", which, index)),
      which,
      table,
      index,
      &[],
    ));
  }
}

fn process_cmd(cmd_name: &str, table: &Table, additional_args: &[String]) {
  if table.contains_key("pre") {
    process_pre_post_cmd("pre", cmd_name, &table);
  }

  println!("Running command {}", cmd_name);
  run_cmd(process_args(
    table["command"].as_array().expect(&format!("{}: missing command array", cmd_name)),
    "primary",
    table,
    0,
    additional_args,
  ));

  if table.contains_key("post") {
    process_pre_post_cmd("post", cmd_name, &table);
  }
}

fn primary(cmd_name: &str, additional_args: &[String]) -> Result<(), String> {
  let doc = read_doit_file();
  if doc.contains_key(&cmd_name) {
    if let Some(table) = doc[&cmd_name].as_table() {
      process_cmd(&cmd_name, &table, &additional_args);
    }
    Ok(())
  } else {
    Err(format!("{} not found", cmd_name))
  }
}

fn list_cmds() {
  for (i, (cmd, _)) in read_doit_file().as_table().iter().enumerate() {
    println!("{}: {}", i + 1, cmd);
  }
}

fn print_usage(program: &str, opts: &Options) {
  let brief = format!("Usage: {} <command> [args...]", program);
  println!("{}", opts.usage(&brief));
  println!("Commands are read from {} by default.", DOIT_FILE);
}

fn print_about(program: &str) {
  println!("program: {}", program);
  println!("version: {}", env!("CARGO_PKG_VERSION"));
  println!("author: {}", env!("CARGO_PKG_AUTHORS"));
  println!("about: {}", env!("CARGO_PKG_DESCRIPTION"));
}

fn show_details(cmd_name: &str) -> Result<(), String> {
  let doc = read_doit_file();

  if !doc.contains_key(cmd_name) {
    return Err(format!("{} not found", cmd_name));
  }

  if let Some(table) = doc[cmd_name].as_table() {
    let command = table["command"].as_str().expect("Missing command");
    let description = if table.contains_key("description") {
      table["description"].as_str().unwrap_or("description must be a string")
    } else {
      "No description provided"
    };
    let args = if table.contains_key("args") {
      let toml_args = table["args"].as_array();
      toml_args.iter().map(|arg| render_template(table, &arg.to_string())).collect::<Vec<_>>().join(" ")
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
  Ok(())
}

fn main() {
  let (program, args) = {
    let args0: Vec<_> = env::args().collect();
    let remove = vec!["doit", "do", "--"];
    let args: Vec<_> = args0[1..].iter().skip_while(|x| remove.contains(&x.as_str())).cloned().collect();
    (args0[0].clone(), args)
  };

  let opts = {
    let mut opts = Options::new();
    opts.optflag("", "help", "print this help menu");
    opts.optflag("", "cmds", "list all available commands");
    opts.optflag("", "about", "about this program");
    opts.optopt("", "show", "show details for command", "command");
    opts
  };

  let die = |e: Option<String>| {
    match e {
      | Some(e) => println!("{}", e),
      | None => {}
    }
    print_usage(&program, &opts);
    exit(1);
  };

  let matches = match opts.parse(&args) {
    | Ok(m) => m,
    | Err(e) => die(Some(e.to_string())),
  };

  if matches.opt_present("help") {
    return print_usage(&program, &opts);
  }

  if matches.opt_present("about") {
    return print_about(&program);
  }

  if let Some(cmd_name) = matches.opt_str("show") {
    match show_details(&cmd_name) {
      | Ok(()) => return,
      | Err(e) => die(Some(e)),
    };
  }

  if matches.opt_present("cmds") {
    return list_cmds();
  }
  let cmd_name = matches
    .free
    .get(0)
    .unwrap_or_else(|| {
      die(None);
      &EMPTY_STRING
    })
    .clone();

  let additional_args = if matches.free.len() > 1 { matches.free[1..].to_vec() } else { vec![] };
  match primary(&cmd_name, &additional_args) {
    | Ok(()) => return,
    | Err(e) => die(Some(e)),
  };
}
