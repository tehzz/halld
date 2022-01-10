use halld::LinkerConfig;
use object::{
    write::{self, StandardSegment},
    Architecture, BinaryFormat, Endianness, SectionKind,
};
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};

mod pass1;
mod pass2;

pub(crate) fn run(
    config: PathBuf,
    search: Option<Vec<PathBuf>>,
    output: Option<PathBuf>,
) -> Result<()> {
    let rdr = BufReader::new(
        File::open(&config)
            .with_context(|| format!("couldn't open config script at <{}>", config.display()))?,
    );

    let config: LinkerConfig = serde_json::from_reader(rdr).context("parsing config JSON")?;
    let LinkerConfig {
        mut settings,
        script,
    } = config;

    let config_output = settings.as_mut().and_then(|s| s.output.take());
    let config_search = settings.and_then(|s| s.search_dirs);

    let output = output
        .or(config_output)
        .ok_or_else(|| anyhow!("no output location from JSON or from CLI"))?;

    let search_dirs = match (search, config_search) {
        (Some(s), None) | (None, Some(s)) => Some(s),
        (Some(mut a), Some(b)) => {
            a.extend(b);
            Some(a)
        }
        (None, None) => None,
    };

    let p1 = pass1::Pass1::run(script, search_dirs).context("linker pass 1")?;
    let p2 = pass2::Pass2::run(p1)?;
    println!("c header\n{:#?}", &p2.c_header);
    let obj = create_object(p2);

    let wtr = BufWriter::new(File::create(output).context("making output file")?);

    obj.write_stream(wtr).expect("writing output object file");

    Ok(())
}

fn is_object(p: impl AsRef<Path>) -> bool {
    // todo: replace with something that checks for relocatable object?
    p.as_ref().extension().map_or(false, |ex| ex == "o")
}

fn create_object(p2: pass2::Pass2) -> write::Object<'static> {
    let mut obj = write::Object::new(BinaryFormat::Elf, Architecture::Mips64, Endianness::Big);

    let data_seg = obj.segment_name(StandardSegment::Data);
    let pass2::Pass2 { table, data, .. } = p2;
    let tsec = obj.add_section(data_seg.to_vec(), b".filetable".to_vec(), SectionKind::Data);
    let fsec = obj.add_section(data_seg.to_vec(), b".files".to_vec(), SectionKind::Data);

    obj.set_section_data(tsec, table, 4);
    obj.set_section_data(fsec, data, 4);

    obj
}
