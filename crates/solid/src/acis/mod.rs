//! ACIS, the solid modeller data AutoCAD keeps in DWG and DXF files for 3DSOLID, REGION and BODY
//! entities: SAT text up to AutoCAD 2010, SAB binary in the data store from AutoCAD 2013
//! (docs/solids.md, "The ACIS translator").
//!
//! - [`read`] turns ACIS data, in any dialect AutoCAD writes (Spatial's ACIS save formats and
//!   Autodesk's ShapeManager ones, text or binary, scrambled or plain), into kernel bodies.
//! - [`write_sat`] writes a body as SAT save format 400, which every AutoCAD reads from DWG and
//!   DXF 2000 to 2010; [`write_sab`] as SAB save format 700, for the data store of 2013 and later.
//!
//! Both go through STEP, which solidcore reads and writes and which describes a B-rep the same
//! way ACIS does: reading, `cadmpeg-asm` (Apache-2.0) decodes the ACIS records into a neutral
//! B-rep that becomes STEP entities; writing, solidcore's STEP entities become ACIS records.

mod model;
mod read;
mod write;

pub use model::scramble;
pub use read::{MAX_DATA, read};

use crate::body::{Body, Result};

/// The body as SAT text (save format 400), header and end line included, one record a line.
pub fn write_sat(body: &Body) -> Result<String> {
    Ok(model::sat(&write::body_records(body)?, 1))
}

/// The body as SAB (save format 700): what an AutoCAD 2013 and later data store record holds.
pub fn write_sab(body: &Body) -> Result<Vec<u8>> {
    Ok(model::sab(&write::body_records(body)?, 1))
}

#[cfg(test)]
mod tests;
