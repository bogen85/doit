// code: language=Rust insertSpaces=true tabSize=2
use dirs;
use getopts::Options;
use once_cell::sync::Lazy;
use regex::Regex;
use std::{
  env,
  fs::{write as write_file, File},
  io::Read,
  path::Path,
  process::{exit, Command},
};
use toml_edit::{Array, Document, Table};
use users::{get_user_by_name, os::unix::UserExt};

const ASCII_SUB1: &str = "\x1A\x01";

static HOME: Lazy<String> =
  Lazy::new(|| format!("{}/", dirs::home_dir().expect("home_dir undefined").to_str().expect("String").to_string()));

static ENV0_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"%env:(.*?):(.*?)%").unwrap());
static ENV1_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"%env:(.*?)%").unwrap());
static VAR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"%(.*?)%").unwrap());
static TILDE_USER_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"~([a-z_][a-z0-9_-]{0,30})?/").unwrap());
static SECTION_KEY_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^@(\d+)$").unwrap());

const DOIT_FILE: &str = "doit.toml";

const DEFAULT_COMMANDS: &str = include_str!("../default_commands.toml");

fn read_doit_file() -> Result<Document, String> {
  let full_contents = if Path::new(DOIT_FILE).exists() {
    let mut contents = String::default();

    File::open(DOIT_FILE).map_err(|e| e.to_string())?.read_to_string(&mut contents).map_err(|e| e.to_string())?;

    format!("{}{}", DEFAULT_COMMANDS, contents)
  } else {
    DEFAULT_COMMANDS.to_string()
  };
  Ok(full_contents.parse::<Document>().map_err(|e| e.to_string())?)
}

fn get_section<'a>(doc: &'a Document, name: &'a str) -> Result<(Option<&'a Table>, String), String> {
  if let Some(caps) = SECTION_KEY_RE.captures(name) {
    Ok({
      let mut actual_key = String::default();
      (
        doc
          .as_table()
          .iter()
          .nth(caps.get(1).ok_or("RE failed")?.as_str().parse::<usize>().ok().ok_or("INDEX")? - 1)
          .and_then(|(key, section)| {
            actual_key = key.to_string();
            section.as_table()
          }),
        actual_key,
      )
    })
  } else {
    if doc.contains_key(name) {
      Ok((doc[name].as_table(), name.to_string()))
    } else {
      Err(format!("{} not found in the {}", name, DOIT_FILE))
    }
  }
}

fn render_template(table: &Table, template: &str) -> Result<String, String> {
  if template.is_empty() {
    return Ok(template.to_string());
  }

  let x1 = {
    match &template[..1] {
      ":" => {
        let x0 = &template[1..];
        if x0.is_empty() {
          return Ok(x0.to_string());
        }
        x0
      }
      _ => return Ok(template.to_string()),
    }
    .replace("%%", &ASCII_SUB1)
  };

  let x2 = ENV0_RE.replace_all(&x1, |caps: &regex::Captures| {
    let evar = &caps[1];
    match env::var(&evar) {
      Ok(value) => value,
      Err(_) => caps[2].to_string(),
    }
  });

  let errors = std::cell::RefCell::new(Vec::<String>::new());
  let push_error = |e: String| -> String {
    errors.borrow_mut().push(e);
    String::default()
  };

  let x3 = ENV1_RE.replace_all(&x2, |caps: &regex::Captures| {
    let evar = &caps[1];
    match env::var(&evar) {
      Ok(value) => value,
      Err(e) => push_error(format!("(Unknown ENV variable: {}: {}", evar, e)),
    }
  });

  let x4 = VAR_RE.replace_all(&x3, |caps: &regex::Captures| {
    let key = &caps[1];

    match table.get(key) {
      None => push_error(format!("(Unknown table key: {})", key)),
      Some(value) => match value.as_str() {
        Some(str_value) => str_value.to_string(),
        None => push_error(format!("(Failed to convert value to string for key: {})", key)),
      },
    }
  });

  let x5 = TILDE_USER_RE.replace_all(&x4, |caps: &regex::Captures| match caps.get(1) {
    None => HOME.to_string(),
    Some(matched) => {
      let username = matched.as_str();
      format!(
        "{}/",
        match get_user_by_name(username) {
          None => push_error(format!("user '{}' not found!", username)),
          Some(user) => user.home_dir().display().to_string(),
        }
      )
    }
  });
  if !errors.borrow().is_empty() {
    return Err(errors.borrow().join("\n"));
  }
  Ok(x5.replace(&ASCII_SUB1, "%"))
}

fn run_builtin(cmd: &str, args: &[String]) -> Result<(), String> {
  println!("builtin: {}: {:?}", cmd, args);
  match cmd {
    "write-file" => {
      let data = "some content";
      write_file("some-file", data).expect("Unable to write file");
      Ok(())
    }
    _ => Err(format!("{} is not a known builtin.", cmd)),
  }
}

fn run_cmd(args: Vec<String>) -> Result<(), String> {
  if args.is_empty() || &args[0] == "#" {
    return Ok(());
  }

  let ignore_rc = args[0] == "-rc";
  if ignore_rc && (args.len() == 1) {
    return Ok(());
  }

  let (cmd, argv) = if ignore_rc { (&args[1], &args[2..]) } else { (&args[0], &args[1..]) };

  match cmd.as_str() {
    cmd if cmd.starts_with("&") => run_builtin(&cmd[1..], argv),
    _ => {
      let mut child = Command::new(cmd).args(argv).spawn().map_err(|e| e.to_string())?;
      let exit_status = child.wait();

      let rc = if ignore_rc { 0 } else { exit_status.map_err(|e| e.to_string())?.code().unwrap_or(1) };
      if rc != 0 {
        Err(format!("{:?}\nfailed with exit status: {}", args, rc))
      } else {
        Ok(())
      }
    }
  }
}

fn run_argv(vec_in: &Array, which: &str, table: &Table, index: usize, args: &[String]) -> Result<(), String> {
  run_cmd({
    if vec_in.len() < 1 {
      return Err(format!("{}[{}] arg vector is empty", which, index));
    }
    let mut vec: Vec<String> = Vec::new();

    for arg in vec_in {
      vec.push(render_template(
        table,
        match &arg.as_str() {
          Some(x) => x,
          None => {
            return Err(format!("Unable to extract argument {} as a string", arg));
          }
        },
      )?);
    }
    vec.extend_from_slice(args);
    vec
  })
}

fn process_pre_post_cmd(which: &str, cmd_name: &str, table: &Table) -> Result<(), String> {
  let sub_args = match table[which].as_array() {
    Some(args) => args,
    None => {
      return Err(format!("{} is not an array", which));
    }
  };

  for (index, args_in) in sub_args.iter().enumerate() {
    println!("Running command {}:{}:{}", cmd_name, which, index + 1);
    run_argv(
      match args_in.as_array() {
        Some(args) => args,
        None => {
          return Err(format!("{}[{}] is not an array", which, index));
        }
      },
      which,
      table,
      index,
      &[],
    )?;
  }
  Ok(())
}

fn get_command<'a>(cmd_name: &str, table: &'a Table) -> Result<&'a Array, String> {
  table
    .get("command")
    .ok_or_else(|| format!("{}: missing command array", cmd_name))
    .and_then(|argv| argv.as_array().ok_or_else(|| format!("{}: command is not an array", cmd_name)))
}

fn process_cmd(cmd_name: &str, table: &Table, args: &[String]) -> Result<(), String> {
  if table.contains_key("pre") {
    process_pre_post_cmd("pre", cmd_name, &table)?;
  }

  println!("Running command {}", cmd_name);
  run_argv(get_command(cmd_name, table)?, "main", table, 0, args)?;

  if table.contains_key("post") {
    process_pre_post_cmd("post", cmd_name, &table)?;
  }
  Ok(())
}

fn primary(cmd_name: &str, args: &[String]) -> Result<(), String> {
  let doc = read_doit_file()?;
  match get_section(&doc, cmd_name) {
    Ok((Some(table), actual_cmd)) => process_cmd(&actual_cmd, &table, &args),
    Err(e) => Err(format!("{} not found: {}", cmd_name, e)),
    Ok((None, _)) => Err(format!("{} not found", cmd_name)),
  }
}

fn list_cmds() -> Result<(), String> {
  let doc = read_doit_file()?;
  for (i, (cmd, _)) in doc.as_table().iter().enumerate() {
    println!("@{} : {}", i + 1, cmd);
  }
  Ok(())
}

fn print_usage(program: &str, opts: &Options) -> Result<(), String> {
  let brief = format!("Usage: {} <command> [args...]", program);
  println!("{}", opts.usage(&brief));
  println!("Commands are read from {} by default.", DOIT_FILE);
  Ok(())
}

fn print_about(program: &str) -> Result<(), String> {
  println!(
    "program: {}\nversion: {}\nauthor: {}\nabout: {}",
    program,
    env!("CARGO_PKG_VERSION"),
    env!("CARGO_PKG_AUTHORS"),
    env!("CARGO_PKG_DESCRIPTION")
  );
  Ok(())
}

fn show_details(cmd_name: &str) -> Result<(), String> {
  let doc = read_doit_file()?;

  let mut errors = Vec::<String>::new();
  match get_section(&doc, cmd_name) {
    Ok((Some(table), actual_cmd)) => {
      let command = get_command(cmd_name, table)?;

      let description = table
        .get("description")
        .ok_or_else(|| "No description provided".to_string())
        .and_then(|x| x.as_str().ok_or_else(|| "description must be a string".to_string()))?;

      let args = if table.contains_key("args") {
        let toml_args = table["args"].as_array();
        toml_args
          .iter()
          .map(|arg| match render_template(table, &arg.to_string()) {
            Ok(s) => s,
            Err(e) => {
              errors.push(format!("{}", e));
              "????".to_string()
            }
          })
          .collect::<Vec<_>>()
          .join(" ")
      } else {
        String::default()
      };
      println!(
        "Given: {}\nActual: {}\nCommand: {}\nArguments:{}\nDescription: {}\n",
        cmd_name, actual_cmd, command, args, description
      );
    }
    Ok((None, _)) => errors.push(format!("Command {} not found", cmd_name)),
    Err(e) => errors.push(format!("Command {} not found: {}", cmd_name, e)),
  }
  if !errors.is_empty() {
    Err(errors.join("\n"))
  } else {
    Ok(())
  }
}

fn main() -> Result<(), String> {
  let (program, args) = {
    let args0: Vec<_> = env::args().collect();
    let remove = vec!["doit", "do", "--"];
    let args: Vec<_> = args0[1..].iter().skip_while(|x| remove.contains(&x.as_str())).cloned().collect();
    (args0[0].clone(), args)
  };

  let opts = {
    let mut opt = Options::new();
    opt.optflag("", "help", "print this help menu");
    opt.optflag("", "cmds", "list all available commands");
    opt.optflag("", "about", "about this program");
    opt.optopt("", "show", "show details for command", "command");
    opt
  };

  let die = |e: Option<String>| {
    if let Some(e) = e {
      println!("{}", e);
    }
    let _ = print_usage(&program, &opts);
    exit(1);
  };

  let matches = match opts.parse(&args) {
    Ok(m) => m,
    Err(e) => die(Some(e.to_string())),
  };

  if matches.opt_present("help") {
    return print_usage(&program, &opts);
  }

  if matches.opt_present("about") {
    return print_about(&program);
  }

  if let Some(cmd_name) = matches.opt_str("show") {
    match show_details(&cmd_name) {
      Ok(()) => return Ok(()),
      Err(e) => die(Some(e)),
    };
  }

  if matches.opt_present("cmds") {
    match list_cmds() {
      Ok(()) => return Ok(()),
      Err(e) => die(Some(e)),
    };
  }
  let empty = String::default();
  let cmd_name = matches
    .free
    .get(0)
    .unwrap_or_else(|| {
      die(None);
      &empty
    })
    .clone();

  let args = if matches.free.len() > 1 { matches.free[1..].to_vec() } else { vec![] };
  if let Err(e) = primary(&cmd_name, &args) {
    die(Some(e));
  }
  Ok(())
}
