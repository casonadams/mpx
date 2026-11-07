//! The option grammar the command parsers share: which flags are switches or take a value, and
//! how the words after them are read.
use crate::protocol::Side;

/// Option shape for one command: which flags are switches, which take a value, and whether
/// option parsing stops at the first positional word (for trailing command lines and keys).
#[derive(Clone, Copy)]
pub(super) struct Spec {
    bools: &'static [&'static str],
    values: &'static [&'static str],
    stop_at_arg: bool,
}

impl Spec {
    /// Options that may be followed by operands, which are collected in `Opts::rest`.
    pub(super) const fn flags(
        bools: &'static [&'static str],
        values: &'static [&'static str],
    ) -> Self {
        Self {
            bools,
            values,
            stop_at_arg: false,
        }
    }

    /// Options up to the first operand; everything from there on is the operands (a trailing
    /// command line, keys or a value).
    pub(super) const fn until_operand(
        bools: &'static [&'static str],
        values: &'static [&'static str],
    ) -> Self {
        Self {
            bools,
            values,
            stop_at_arg: true,
        }
    }
}

/// The `-t` target alone, for commands that act on one target.
pub(super) const TARGET: Spec = Spec::flags(&[], &["-t"]);

/// Whether a command takes words after its options.
#[derive(Clone, Copy)]
pub(super) enum Operands {
    /// Any word left after the options is an error, reported before the command is built.
    Refuse,
    /// The builder takes the words left after the options (`Opts::rest`).
    Allow,
    /// At least one word must be left; the message says what is missing.
    Require(&'static str),
}

/// Parses `args` against `spec`, checks the operands, and builds the command from the options.
pub(super) fn with_opts<T>(
    name: &str,
    args: &[String],
    spec: Spec,
    operands: Operands,
    build: impl FnOnce(Opts) -> Result<T, String>,
) -> Result<T, String> {
    let o = parse_opts(name, args, spec)?;
    match operands {
        Operands::Refuse => o.no_rest(name)?,
        Operands::Allow => {}
        Operands::Require(missing) if o.rest.is_empty() => {
            return Err(format!("{name}: {missing}"));
        }
        Operands::Require(_) => {}
    }
    build(o)
}

#[derive(Debug, Default)]
pub(super) struct Opts {
    flags: Vec<String>,
    values: Vec<(String, String)>,
    pub(super) rest: Vec<String>,
}

impl Opts {
    pub(super) fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    pub(super) fn value(&self, flag: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(f, _)| f == flag)
            .map(|(_, v)| v.as_str())
    }

    pub(super) fn string(&self, flag: &str) -> Option<String> {
        self.value(flag).map(str::to_string)
    }

    pub(super) fn no_rest(&self, name: &str) -> Result<(), String> {
        match self.rest.first() {
            Some(extra) => Err(format!("{name}: unexpected argument '{extra}'")),
            None => Ok(()),
        }
    }

    /// The `-t` value, else the first operand; `missing` completes the error when neither is set.
    pub(super) fn target_or_first(&self, name: &str, missing: &str) -> Result<String, String> {
        self.string("-t")
            .or_else(|| self.rest.first().cloned())
            .ok_or_else(|| format!("{name}: {missing}"))
    }
}

pub(super) fn parse_opts(name: &str, args: &[String], spec: Spec) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        i += 1;
        if a == "--" {
            o.rest.extend_from_slice(&args[i..]);
            break;
        }
        if !is_flag(a) {
            o.rest.push(a.to_string());
            if spec.stop_at_arg {
                o.rest.extend_from_slice(&args[i..]);
                break;
            }
        } else if spec.values.contains(&a) {
            let v = args
                .get(i)
                .ok_or_else(|| format!("{name}: {a} needs a value"))?;
            i += 1;
            o.values.push((a.to_string(), v.clone()));
        } else {
            take_switches(name, a, spec.bools, &mut o.flags)?;
        }
    }
    Ok(o)
}

fn is_flag(a: &str) -> bool {
    a.len() > 1 && a.starts_with('-')
}

/// `--long` is one switch; `-abc` is three short switches.
fn take_switches(name: &str, a: &str, bools: &[&str], out: &mut Vec<String>) -> Result<(), String> {
    if a.starts_with("--") {
        return if bools.contains(&a) {
            out.push(a.to_string());
            Ok(())
        } else {
            Err(format!("{name}: unknown option {a}"))
        };
    }
    for ch in a[1..].chars() {
        let flag = format!("-{ch}");
        if !bools.contains(&flag.as_str()) {
            return Err(format!("{name}: unknown option {flag}"));
        }
        out.push(flag);
    }
    Ok(())
}

/// The side named by the one of `-L`, `-R`, `-U`, `-D` that is set.
pub(super) fn direction_flag(o: &Opts) -> Option<Side> {
    [
        ("-L", Side::Left),
        ("-R", Side::Right),
        ("-U", Side::Up),
        ("-D", Side::Down),
    ]
    .into_iter()
    .find(|(flag, _)| o.has(flag))
    .map(|(_, side)| side)
}
