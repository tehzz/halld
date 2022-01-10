use std::{collections::HashMap, fs, path::{PathBuf, Path, Component}};

use crate::link;
use halld::LinkerScript;

use anyhow::{bail, Context, Result};
use object::{read, Object, ObjectSymbol, SymbolKind};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct Sym {
    pub(crate) addr: u32,
    pub(crate) file: usize,
}

pub(crate) type SymMap = HashMap<String, Sym>;
pub(crate) type CDefs = Vec<(String, u16)>;

#[derive(Debug)]
pub(crate) struct Pass1 {
    pub(crate) script: LinkerScript,
    pub(crate) sym_map: SymMap,
    pub(crate) c_header: CDefs,
}

impl Pass1 {
    pub(crate) fn run(mut script: LinkerScript, search_dirs: Option<Vec<PathBuf>>) -> Result<Self> {
        let search = search_dirs.as_deref();

        let mut sym_map = HashMap::with_capacity(script.len());
        let mut c_header = Vec::with_capacity(script.len());
        let mut sym_rename = None;
        for (i, entry) in script.iter_mut().enumerate() {
            // use the original name for creating c defines
            let idx = u16::try_from(i)
                .with_context(|| format!("More than u16::MAX files: file <{}> was {} of max {}", entry.file.display(), i, u16::MAX))?;
            let def = (fmt_filename(&entry.file), idx);
            c_header.push(def); 
            // what to do about the same named files...?
            locate_file(&mut entry.file, search).context("locating files to link")?;

            if link::is_object(&entry.file) {
                let file = fs::read(&entry.file)?;
                let obj = read::File::parse(&*file)?;
                for sym in obj.symbols() {
                    if sym.kind() == SymbolKind::Unknown && sym.is_global() {
                        let name = sym.name()?.to_string();
                        let addr = sym.address() as u32;
                        sym_map.insert(
                            name,
                            Sym {
                                addr,
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

        Ok(Self { script, sym_map, c_header })
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

fn fmt_filename(p: &Path) -> String {
    let mut s = "RLD_FID".to_string();
    if let Some(parent) = p.parent() {
        for cmpt in parent.components() {
            s += "_";
            match cmpt {
                Component::Normal(p) => s += &p.to_ascii_uppercase().to_string_lossy(),
                Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir => (),
            }
        }
    }

    if let Some(stem) = p.file_stem() {
        s += "_";
        s += &stem.to_ascii_uppercase().to_string_lossy();
    }

    s
}
