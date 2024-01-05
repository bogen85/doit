// code: language=Rust insertSpaces=true tabSize=2
use dirs;
use getopts::Options;
use once_cell::sync::Lazy;
use regex::Regex;
use std::process::{exit, Command};
use std::{env, fs::File, io::Read};
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

fn read_doit_file() -> Result<Document, String> {
  let mut contents = String::default();
  File::open(DOIT_FILE).expect("Unable to open file").read_to_string(&mut contents).expect("Unable to read file");
  Ok(contents.parse::<Document>().expect("Unable to parse TOML"))
}

fn get_section<'a>(doc: &'a Document, name: &'a str) -> Result<Option<&'a Table>, String> {
  if let Some(caps) = SECTION_KEY_RE.captures(name) {
    Ok(
      doc
        .as_table()
        .iter()
        .nth(caps.get(1).ok_or("RE failed")?.as_str().parse::<usize>().ok().ok_or("INDEX")? - 1)
        .and_then(|(_, section)| section.as_table()),
    )
  } else {
    if doc.contains_key(name) {
      Ok(doc[name].as_table())
    } else {
      Err(format!("{} not found in the {}", name, DOIT_FILE))
    }
  }
}

fn render_template(table: &Table, template: &str) -> Result<String, String> {
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
      Some(value) => format!("{}", value.as_str().expect("String")),
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

fn run_cmd(args: Vec<String>) -> Result<(), String> {
  let exit_status = {
    let mut child =
      Command::new(&args[0]).args(&args[1..]).spawn().expect(&format!("Failed to execute command: {:?}", args));
    child.wait()
  };
  let rc = exit_status.expect("RC").code().unwrap_or(1);
  if rc != 0 {
    Err(format!("{:?}\nfailed with exit status: {}", args, rc))
  } else {
    Ok(())
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
        &arg.as_str().expect(&format!("Invalid argument for {}:[{}]", which, index)).to_string(),
      )?);
    }
    vec.extend_from_slice(args);
    vec
  })
}

fn process_pre_post_cmd(which: &str, cmd_name: &str, table: &Table) -> Result<(), String> {
  let sub_args = table[which].as_array().expect(&format!("{} is not an array", which));

  for (index, args_in) in sub_args.iter().enumerate() {
    println!("Running command {}:{}:{}", cmd_name, which, index + 1);
    run_argv(args_in.as_array().expect(&format!("{}[{}] is not an array", which, index)), which, table, index, &[])?;
  }
  Ok(())
}

fn process_cmd(cmd_name: &str, table: &Table, args: &[String]) -> Result<(), String> {
  if table.contains_key("pre") {
    process_pre_post_cmd("pre", cmd_name, &table)?;
  }

  println!("Running command {}", cmd_name);
  run_argv(
    table["command"].as_array().expect(&format!("{}: missing command array", cmd_name)),
    "main",
    table,
    0,
    args,
  )?;

  if table.contains_key("post") {
    process_pre_post_cmd("post", cmd_name, &table)?;
  }
  Ok(())
}

fn primary(cmd_name: &str, args: &[String]) -> Result<(), String> {
  let doc = read_doit_file()?;
  match get_section(&doc, cmd_name) {
    Ok(Some(table)) => process_cmd(&cmd_name, &table, &args),
    Err(e) => Err(format!("{} not found: {}", cmd_name, e)),
    Ok(None) => Err(format!("{} not found", cmd_name)),
  }
}

fn list_cmds() -> Result<(), String> {
  let doc = read_doit_file()?;
  for (i, (cmd, _)) in doc.as_table().iter().enumerate() {
    println!("{}: {}", i + 1, cmd);
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
    Ok(Some(table)) => {
      let command = table["command"].as_array().expect("Missing command");

      let description = if table.contains_key("description") {
        table["description"].as_str().unwrap_or("description must be a string")
      } else {
        "No description provided"
      };

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
      println!("Alias: {}\nCommand: {}\nArguments:{}\nDescription: {}\n", cmd_name, command, args, description);
    }
    Ok(None) => errors.push(format!("Command {} not found", cmd_name)),
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
