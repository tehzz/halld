use std::{collections::HashMap, fs, path::PathBuf};

use crate::link;
use halld::LinkerScript;

use anyhow::{bail, Context, Result};
use object::{read, Object, ObjectSymbol, SymbolKind};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Sym {
    pub(crate) addr: u32,
    pub(crate) file: usize,
    pub(crate) global: bool,
}

pub(crate) type SymMap = HashMap<String, Sym>;

#[derive(Debug)]
pub(crate) struct Pass1 {
    pub(crate) script: LinkerScript,
    pub(crate) sym_map: HashMap<String, Sym>,
}

impl Pass1 {
    pub(crate) fn run(mut script: LinkerScript, search_dirs: Option<Vec<PathBuf>>) -> Result<Self> {
        let search = search_dirs.as_deref();

        let mut sym_map = HashMap::with_capacity(script.len());
        let mut sym_rename = None;
        for (i, entry) in script.iter_mut().enumerate() {
            // store the original name for easy linking
            // what to do about the same named files...?
            locate_file(&mut entry.file, search).context("locating files to link")?;

            if link::is_object(&entry.file) {
                let file = fs::read(&entry.file)?;
                let obj = read::File::parse(&*file)?;
                for sym in obj.symbols() {
                    if sym.kind() == SymbolKind::Unknown {
                        let name = sym.name()?.to_string();
                        let addr = sym.address() as u32;
                        let global = sym.is_global();
                        sym_map.insert(
                            name,
                            Sym {
                                addr,
                                global,
                                file: i,
                            },
                        );
                    } else {
                        println!("unneeded symbol? {:#?}", sym);
                    }
                }
            } else if let Some(syms) = entry.exports.as_ref() {
                for (name, addr) in syms.iter() {
                    let sym = Sym {
                        addr: *addr,
                        file: i,
                        global: true,
                    };

                    if let Some(old) = sym_map.insert(name.clone(), sym) {
                        sym_rename = Some((name.clone(), old));
                        break;
                    }
                }
            }
        }

        if let Some((name, sym)) = sym_rename {
            bail!(
                "Symobl < {} > already definied in file < {} >",
                name,
                script[sym.file].file.display()
            );
        }

        println!("{:#?}", &sym_map);

        Ok(Self { script, sym_map })
    }
}

fn locate_file(file: &mut PathBuf, search_dirs: Option<&[PathBuf]>) -> Result<()> {
    if fs::metadata(&file).map_or(false, |m| m.is_file()) {
        return Ok(());
    }

    let new_file = search_dirs.and_then(|stems| {
        stems
            .iter()
            .map(|s| s.join(file.as_path()))
            .find(|f| fs::metadata(&f).map_or(false, |m| m.is_file()))
    });

    if let Some(new_file) = new_file {
        *file = new_file;
    } else {
        bail!(
            "Couldn't locate < {} > in current working directory or search dirs < {:?} >",
            file.display(),
            search_dirs
        );
    }

    Ok(())
}
