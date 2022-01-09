use super::pass1::{Pass1, SymMap};
use crate::link;

use std::{fs, path::Path};

use anyhow::{anyhow, bail, Context, Result};
use halld::VpkSettings;
use object::{read, Object, ObjectSection, ObjectSymbol, RelocationTarget};
use vpk0::{format::VpkMethod, Encoder};

#[derive(Debug, Copy, Clone)]
pub(crate) struct FileInfo {
    pub(crate) offset: u32,
    pub(crate) size: u32,
    pub(crate) comp_size: Option<u32>,
    pub(crate) inreloc: Option<u32>,
    pub(crate) exreloc: Option<u32>,
}

#[derive(Debug)]
pub(crate) struct Pass2 {
    pub(crate) table: Vec<u8>,
    pub(crate) data: Vec<u8>,
    pub(crate) symbols: Vec<(String, u32)>,
}

impl Pass2 {
    pub(crate) fn run(pass1: Pass1) -> Result<Self> {
        let Pass1 { script, sym_map } = pass1;

        let mut output = Vec::with_capacity(0x0100_0000);
        let mut table = Vec::with_capacity((script.len() + 1) * 12);
        let mut symbols = Vec::with_capacity(script.len());
        for (i, entry) in script.into_iter().enumerate() {
            let (mut data, externs) = if link::is_object(&entry.file) {
                relocate_obj(&entry.file, &sym_map)
                    .with_context(|| format!("relocating < {} >", entry.file.display()))?
            } else {
                let data = fs::read(&entry.file)
                    .with_context(|| format!("reading < {} > in pass 2", entry.file.display()))?;
                let externs = entry.imports;

                (data, externs)
            };

            align_buffer(&mut data);
            let size = u32::try_from(data.len())?;

            let (data, comp_size) = if entry.compressed {
                let settings = entry.comp_settings.as_ref();
                let mut d = compress_data(data, settings)
                    .with_context(|| format!("compressing <{}>", entry.file.display()))?;

                align_buffer(&mut d);
                let size = u32::try_from(d.len())?;

                (d, Some(size))
            } else {
                (data, None)
            };

            let offset = u32::try_from(output.len())?;
            let info = FileInfo {
                offset,
                size,
                comp_size,
                inreloc: None,
                exreloc: None,
            };
            println!("{}\t{:x?}", i, info);

            add_file_data(&mut output, &data);
            if let Some(ex) = externs.as_deref() {
                add_externs(&mut output, ex);
            }
            add_file_info(&mut table, info).context("writing file info to file table")?;

            symbols.push((format!("RLD_FILE_{}", entry.file.display()), i as u32));
        }

        terminate_table(&mut table, output.len()).context("terminating resource table")?;

        Ok(Self {
            table,
            data: output,
            symbols,
        })
    }
}

/// Right now, this only extracts and relocates data from the .data section of an object
fn relocate_obj(p: &Path, sym_map: &SymMap) -> Result<(Vec<u8>, Option<Vec<u16>>)> {
    let file = fs::read(p).context("opening object for relocation")?;
    let obj = read::File::parse(&*file).context("parsing object for relocation")?;
    let data_sec = obj
        .section_by_name(".data")
        .ok_or_else(|| anyhow!("missing .data section"))?;

    // might be able to make this a Cow
    let mut data = data_sec.data().context("reading .data binary")?.to_vec();
    let mut externs = Vec::new();

    // separate internal and external relocations
    let mut internal_relocs = Vec::with_capacity(16);
    let mut external_relocs = Vec::with_capacity(16);
    for (loc, reloc) in data_sec.relocations() {
        let loc = loc as usize;
        if reloc.size() != 32 {
            bail!("Can only relocate 32bit pointers; {:?}", reloc);
        }
        let sym_name = match reloc.target() {
            RelocationTarget::Symbol(idx) => obj.symbol_by_index(idx)?.name()?,
            unk @ _ => bail!("Unexpected relocation target: {:?}", unk),
        };

        if sym_name == ".data" {
            // internal relocation? gas seems to set the symbol to the section name
            let val = &data[loc..loc + 4];
            let r = (loc, u32::from_be_bytes(val.try_into()?));
            internal_relocs.push(r);
        } else {
            let sym = sym_map.get(sym_name).ok_or_else(|| {
                anyhow!(
                    "couldn't find external symbol <{}> for relocation",
                    sym_name
                )
            })?;
            let val = sym.addr;
            external_relocs.push((loc, val));
            externs.push(sym.file as u16);
        }
    }

    // apply relocations for each
    relocate(&mut data, &internal_relocs)
        .with_context(|| format!("internal relocations in {}", p.display()))?;
    relocate(&mut data, &internal_relocs)
        .with_context(|| format!("external relocations in {}", p.display()))?;

    // return external file ids if there were any
    let externs = if externs.is_empty() {
        None
    } else {
        Some(externs)
    };

    Ok((data, externs))
}

fn relocate(buf: &mut Vec<u8>, relocations: &[(usize, u32)]) -> Result<()> {
    let mut iter = relocations.iter().copied().peekable();

    while let Some(reloc) = iter.next() {
        let next = iter.peek().map(|(l, _)| *l as u32);
        apply_relocation(buf, reloc, next).context("applying relocation")?;
    }

    Ok(())
}

fn apply_relocation(buf: &mut Vec<u8>, (loc, val): (usize, u32), next: Option<u32>) -> Result<()> {
    let ptr = buf
        .get_mut(loc..loc + 4)
        .ok_or_else(|| anyhow!("{}-{} was outside of buffer", loc, loc + 4))?;

    let next = opt_shorten(next).context("reducing the pointer to the next relocation")?;
    let val = shorten(val).context("reducing relocation value")?;

    let reloc = (next as u32) << 16 | (val as u32);

    ptr.copy_from_slice(&reloc.to_be_bytes());

    Ok(())
}

fn compress_data(original: Vec<u8>, settings: Option<&VpkSettings>) -> Result<Vec<u8>> {
    let method = settings
        .and_then(|s| s.method)
        .map(|m| match m {
            0 => VpkMethod::OneSample,
            1 => VpkMethod::TwoSample,
            _ => panic!("Unknown method {}", m),
        })
        .unwrap_or(VpkMethod::OneSample);

    Encoder::for_bytes(&original)
        .method(method)
        .optional_offsets(settings.and_then(|s| s.offsets.as_deref()))
        .optional_lengths(settings.and_then(|s| s.lengths.as_deref()))
        .encode_to_vec()
        .map_err(Into::into)
}

fn add_file_data(v: &mut Vec<u8>, data: &[u8]) {
    v.extend_from_slice(data);
    align_buffer(v);
}

fn add_externs(v: &mut Vec<u8>, externs: &[u16]) {
    let be_iter = externs.iter().copied().flat_map(u16::to_be_bytes);
    v.extend(be_iter);
    align_buffer(v);
}

fn align_buffer(v: &mut Vec<u8>) {
    const ALIGNMENT: usize = 4;

    while v.len() % ALIGNMENT != 0 {
        v.push(0);
    }
}

fn add_file_info(table: &mut Vec<u8>, info: FileInfo) -> Result<()> {
    let FileInfo {
        offset,
        size,
        comp_size,
        inreloc,
        exreloc,
    } = info;
    let offset = offset | (comp_size.is_some() as u32) << 31;
    let size = shorten(size).context("size")?;
    let comp_size = opt_shorten(comp_size).context("compressed size")?;
    let inreloc = opt_shorten(inreloc).context("interal relocations start")?;
    let exreloc = opt_shorten(exreloc).context("exteral relocations start")?;

    // entry size is 12 bytes
    let entry = offset
        .to_be_bytes()
        .into_iter()
        .chain(inreloc.to_be_bytes())
        .chain(comp_size.to_be_bytes())
        .chain(exreloc.to_be_bytes())
        .chain(size.to_be_bytes());

    table.extend(entry);

    Ok(())
}

fn terminate_table(table: &mut Vec<u8>, end: usize) -> Result<()> {
    const NULL_ENTRY: &[u8] = &[0; 8];
    let end = u32::try_from(end)?;
    table.extend_from_slice(&end.to_be_bytes());
    table.extend_from_slice(NULL_ENTRY);

    Ok(())
}

/// Reduce a u32 (like an N64 o32 pointer) to a 16bit word offset
fn shorten(x: u32) -> Result<u16> {
    if x % 4 != 0 {
        Err(anyhow!("{} was not in word (four byte) alignment ", x))
    } else {
        u16::try_from(x / 4).with_context(|| format!("{} / 4 = {} is too large for u16", x, x / 4))
    }
}

/// Reduce a u32 nullable value to either x/4 or 0xFFFF
fn opt_shorten(x: Option<u32>) -> Result<u16> {
    x.map_or(Ok(0xFFFF), shorten)
}
