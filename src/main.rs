use anyhow::{anyhow, Result};
use std::{ffi::OsStr, path::PathBuf};

mod link;

fn print_usage() {
    println!(
        "{} <config.json> [-L path/to/search -L another/path] -o <output>",
        env!("CARGO_BIN_NAME")
    );
}

fn print_version() {
    println!("{} - {}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"));
    println!("A linker for HAL's filesystem in SSB64");
}

#[derive(Debug)]
enum Opt {
    Run {
        config: PathBuf,
        search: Option<Vec<PathBuf>>,
        output: Option<PathBuf>,
    },
    Help,
    Version,
}

impl Opt {
    fn from_args() -> Result<Self> {
        let mut args = pico_args::Arguments::from_env();

        if args.contains(["-h", "--help"]) {
            return Ok(Self::Help);
        }

        if args.contains(["-V", "--version"]) {
            return Ok(Self::Version);
        }

        let search = args.values_from_os_str(["-L", "--search-dir"], to_pathbuf)?;
        let search = if search.is_empty() {
            None
        } else {
            Some(search)
        };
        let output = args.opt_value_from_os_str(["-o", "--output"], to_pathbuf)?;

        let config = args
            .finish()
            .into_iter()
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("Path to config JSON file not passed. Use \'-h\' for help"))?;

        Ok(Self::Run {
            config,
            search,
            output,
        })
    }
}

fn main() -> Result<()> {
    let opt = Opt::from_args()?;

    match opt {
        Opt::Help => {
            print_usage();
            Ok(())
        }
        Opt::Version => {
            print_version();
            Ok(())
        }
        Opt::Run {
            config,
            search,
            output,
        } => link::run(config, search, output),
    }
}

fn to_pathbuf(s: &OsStr) -> Result<PathBuf> {
    Ok(PathBuf::from(s))
}

/*
 * Program Overview
 *   1. Decode JSON and CLI settings
 *   2. Pass 1
 *      a. Find files specified in JSON
 *      b. Read file (if object) and add symbols to lookup maps
 *      c. Check for valid values? (e.g., is vpk method valid?)
 *   3. Pass 2
 *      a. Resolve symbols (in object) with lookup maps
 *      b. Create list of necessary imports for each file (if obj)
 *      c. Compress files
 *      d. Stitch together all files and keep track of the beginning and size of files
 *      e. Create resource header
 *   4. Output obj with header and filedata
 */
