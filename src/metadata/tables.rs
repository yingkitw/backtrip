use crate::error::{Error, Result};
use crate::metadata::streams::{decode_compressed_uint, MetadataRoot};

/// ECMA-335 table numbers (II.22).
pub mod tbl {
    pub const MODULE: u8 = 0;
    pub const TYPEREF: u8 = 1;
    pub const TYPEDEF: u8 = 2;
    pub const FIELD: u8 = 4;
    pub const METHODDEF: u8 = 6;
    pub const PARAM: u8 = 8;
    pub const INTERFACEIMPL: u8 = 9;
    pub const MEMBERREF: u8 = 10;
    pub const CONSTANT: u8 = 11;
    pub const CUSTOMATTRIBUTE: u8 = 12;
    pub const FIELDMARSHAL: u8 = 13;
    pub const DECLSECURITY: u8 = 14;
    pub const CLASSLAYOUT: u8 = 15;
    pub const FIELDLAYOUT: u8 = 16;
    pub const STANDALONESIG: u8 = 17;
    pub const EVENTMAP: u8 = 18;
    pub const EVENT: u8 = 20;
    pub const PROPERTYMAP: u8 = 21;
    pub const PROPERTY: u8 = 23;
    pub const METHODSEMANTICS: u8 = 24;
    pub const METHODIMPL: u8 = 25;
    pub const MODULEREF: u8 = 26;
    pub const TYPESPEC: u8 = 27;
    pub const IMPLMAP: u8 = 28;
    pub const FIELDRVA: u8 = 29;
    pub const ASSEMBLY: u8 = 32;
    pub const ASSEMBLYPROCESSOR: u8 = 33;
    pub const ASSEMBLYOS: u8 = 34;
    pub const ASSEMBLYREF: u8 = 35;
    pub const ASSEMBLYREFPROCESSOR: u8 = 36;
    pub const ASSEMBLYREFOS: u8 = 37;
    pub const FILE: u8 = 38;
    pub const EXPORTEDTYPE: u8 = 39;
    pub const MANIFESTRESOURCE: u8 = 40;
    pub const NESTEDCLASS: u8 = 41;
    pub const GENERICPARAM: u8 = 42;
    pub const METHODSPEC: u8 = 43;
    pub const GENERICPARAMCONSTRAINT: u8 = 44;
}

/// Coded index kinds (II.24.2.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coded {
    TypeDefOrRef,
    HasConstant,
    HasCustomAttribute,
    HasFieldMarshal,
    HasDeclSecurity,
    MemberRefParent,
    HasSemantics,
    MethodDefOrRef,
    MemberForwarded,
    Implementation,
    CustomAttributeType,
    ResolutionScope,
    TypeOrMethodDef,
}

impl Coded {
    /// Tables referenced by this coded index, in tag order.
    fn tables(self) -> &'static [Option<u8>] {
        match self {
            Coded::TypeDefOrRef => &[Some(tbl::TYPEDEF), Some(tbl::TYPEREF), Some(tbl::TYPESPEC)],
            Coded::HasConstant => &[Some(tbl::FIELD), Some(tbl::PARAM), Some(tbl::PROPERTY)],
            Coded::HasCustomAttribute => &[
                Some(tbl::METHODDEF), Some(tbl::FIELD), Some(tbl::TYPEREF), Some(tbl::TYPEDEF),
                Some(tbl::PARAM), Some(tbl::INTERFACEIMPL), Some(tbl::MEMBERREF), Some(tbl::MODULE),
                Some(tbl::DECLSECURITY), Some(tbl::PROPERTY), Some(tbl::EVENT), Some(tbl::STANDALONESIG),
                Some(tbl::MODULEREF), Some(tbl::TYPESPEC), Some(tbl::ASSEMBLY), Some(tbl::ASSEMBLYREF),
                Some(tbl::FILE), Some(tbl::EXPORTEDTYPE), Some(tbl::MANIFESTRESOURCE), Some(tbl::GENERICPARAM),
                Some(tbl::GENERICPARAMCONSTRAINT), Some(tbl::METHODSPEC),
            ],
            Coded::HasFieldMarshal => &[Some(tbl::FIELD), Some(tbl::PARAM)],
            Coded::HasDeclSecurity => &[Some(tbl::TYPEDEF), Some(tbl::METHODDEF), Some(tbl::ASSEMBLY)],
            Coded::MemberRefParent => &[
                Some(tbl::TYPEDEF), Some(tbl::TYPEREF), Some(tbl::MODULEREF), Some(tbl::METHODDEF),
                Some(tbl::TYPESPEC),
            ],
            Coded::HasSemantics => &[Some(tbl::EVENT), Some(tbl::PROPERTY)],
            Coded::MethodDefOrRef => &[Some(tbl::METHODDEF), Some(tbl::MEMBERREF)],
            Coded::MemberForwarded => &[Some(tbl::FIELD), Some(tbl::METHODDEF)],
            Coded::Implementation => &[Some(tbl::FILE), Some(tbl::ASSEMBLYREF), Some(tbl::EXPORTEDTYPE)],
            Coded::CustomAttributeType => &[
                None, None, Some(tbl::METHODDEF), Some(tbl::MEMBERREF), None,
            ],
            Coded::ResolutionScope => &[Some(tbl::MODULE), Some(tbl::MODULEREF), Some(tbl::ASSEMBLYREF), Some(tbl::TYPEREF)],
            Coded::TypeOrMethodDef => &[Some(tbl::TYPEDEF), Some(tbl::METHODDEF)],
        }
    }

    fn tag_bits(self) -> u32 {
        let n = self.tables().len();
        match n {
            1 => 0,
            2 => 1,
            3 | 4 => 2,
            5..=8 => 3,
            9..=16 => 4,
            _ => 5,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Col {
    U8,
    U16,
    U32,
    String,
    Guid,
    Blob,
    Table(u8),
    CodedIdx(Coded),
}

/// Schema for a metadata table: ordered list of columns.
pub fn schema(table: u8) -> Option<&'static [Col]> {
    use Col::*;
    Some(match table {
        tbl::MODULE => &[GENERATION, String, Guid, Guid, Guid],
        tbl::TYPEREF => &[CodedIdx(Coded::ResolutionScope), String, String],
        tbl::TYPEDEF => &[U32, String, String, CodedIdx(Coded::TypeDefOrRef), Table(tbl::FIELD), Table(tbl::METHODDEF)],
        tbl::FIELD => &[U16, String, Blob],
        tbl::METHODDEF => &[U32, U16, U16, String, Blob, Table(tbl::PARAM)],
        tbl::PARAM => &[U16, U16, String],
        tbl::INTERFACEIMPL => &[Table(tbl::TYPEDEF), CodedIdx(Coded::TypeDefOrRef)],
        tbl::MEMBERREF => &[CodedIdx(Coded::MemberRefParent), String, Blob],
        tbl::CONSTANT => &[U8, U8, CodedIdx(Coded::HasConstant), Blob],
        tbl::CUSTOMATTRIBUTE => &[CodedIdx(Coded::HasCustomAttribute), CodedIdx(Coded::CustomAttributeType), Blob],
        tbl::FIELDMARSHAL => &[CodedIdx(Coded::HasFieldMarshal), Blob],
        tbl::DECLSECURITY => &[U16, CodedIdx(Coded::HasDeclSecurity), Blob],
        tbl::CLASSLAYOUT => &[U16, U32, Table(tbl::TYPEDEF)],
        tbl::FIELDLAYOUT => &[U32, Table(tbl::FIELD)],
        tbl::STANDALONESIG => &[Blob],
        tbl::EVENTMAP => &[Table(tbl::TYPEDEF), Table(tbl::EVENT)],
        tbl::EVENT => &[U16, String, CodedIdx(Coded::TypeDefOrRef)],
        tbl::PROPERTYMAP => &[Table(tbl::TYPEDEF), Table(tbl::PROPERTY)],
        tbl::PROPERTY => &[U16, String, Blob],
        tbl::METHODSEMANTICS => &[U16, Table(tbl::METHODDEF), CodedIdx(Coded::HasSemantics)],
        tbl::METHODIMPL => &[Table(tbl::TYPEDEF), CodedIdx(Coded::MethodDefOrRef), CodedIdx(Coded::MethodDefOrRef)],
        tbl::MODULEREF => &[String],
        tbl::TYPESPEC => &[Blob],
        tbl::IMPLMAP => &[U16, CodedIdx(Coded::MemberForwarded), String, Table(tbl::MODULEREF)],
        tbl::FIELDRVA => &[U32, Table(tbl::FIELD)],
        tbl::ASSEMBLY => &[U32, U16, U16, U16, U16, U32, Blob, String, String, Blob],
        tbl::ASSEMBLYPROCESSOR => &[U32],
        tbl::ASSEMBLYOS => &[U32, U32, U32],
        tbl::ASSEMBLYREF => &[U16, U16, U16, U16, U32, Blob, String, String, Blob],
        tbl::ASSEMBLYREFPROCESSOR => &[U32, Table(tbl::ASSEMBLYREF)],
        tbl::ASSEMBLYREFOS => &[U32, U32, U32, Table(tbl::ASSEMBLYREF)],
        tbl::FILE => &[U32, String, Blob],
        tbl::EXPORTEDTYPE => &[U32, U32, String, String, CodedIdx(Coded::Implementation)],
        tbl::MANIFESTRESOURCE => &[U32, U32, String, CodedIdx(Coded::Implementation)],
        tbl::NESTEDCLASS => &[Table(tbl::TYPEDEF), Table(tbl::TYPEDEF)],
        tbl::GENERICPARAM => &[U16, U16, CodedIdx(Coded::TypeOrMethodDef), String],
        tbl::METHODSPEC => &[CodedIdx(Coded::MethodDefOrRef), Blob],
        tbl::GENERICPARAMCONSTRAINT => &[Table(tbl::GENERICPARAM), CodedIdx(Coded::TypeDefOrRef)],
        _ => return None,
    })
}

const GENERATION: Col = Col::U16;

/// A decoded coded index: target table (if any) and 1-based row index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodedIndex {
    pub table: Option<u8>,
    pub row: u32, // 1-based
}

/// A parsed metadata row: vector of column values.
#[derive(Debug, Clone)]
pub struct Row {
    pub cols: Vec<u32>,
}

impl Row {
    pub fn col(&self, i: usize) -> u32 {
        self.cols[i]
    }
}

/// Index-size context computed from heap sizes and row counts.
pub struct IndexSizes {
    pub string: usize,
    pub guid: usize,
    pub blob: usize,
    pub rows: [u32; 64],
    pub table_index: [usize; 64], // 2 or 4 bytes for each table index
}

impl IndexSizes {
    fn coded_size(&self, c: Coded) -> usize {
        let tag = c.tag_bits();
        let max_rows = c.tables().iter().filter_map(|t| *t).map(|t| self.rows[t as usize]).max().unwrap_or(0);
        if (max_rows as u64) < (1u64 << (16 - tag)) {
            2
        } else {
            4
        }
    }

    fn col_size(&self, col: Col) -> usize {
        match col {
            Col::U8 => 1,
            Col::U16 => 2,
            Col::U32 => 4,
            Col::String => self.string,
            Col::Guid => self.guid,
            Col::Blob => self.blob,
            Col::Table(t) => self.table_index[t as usize],
            Col::CodedIdx(c) => self.coded_size(c),
        }
    }
}

/// The full set of parsed tables.
pub struct Tables {
    pub rows: Vec<Vec<Row>>,      // indexed by table number; empty if absent
    pub row_counts: [u32; 64],
    pub index_sizes: IndexSizes,
}

impl Tables {
    pub fn get(&self, table: u8) -> &[Row] {
        self.rows.get(table as usize).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn row_count(&self, table: u8) -> u32 {
        self.row_counts[table as usize]
    }
}

pub fn parse_tables(root: &MetadataRoot) -> Result<Tables> {
    let stream = root.tables_stream();
    if stream.len() < 24 {
        return Err(Error::InvalidMetadata("tables stream too small".into()));
    }
    let heap_sizes = stream[6];
    let valid = u64::from_le_bytes(stream[8..16].try_into().unwrap());
    let _sorted = u64::from_le_bytes(stream[16..24].try_into().unwrap());

    let string_sz = if heap_sizes & 0x01 != 0 { 4 } else { 2 };
    let guid_sz = if heap_sizes & 0x02 != 0 { 4 } else { 2 };
    let blob_sz = if heap_sizes & 0x04 != 0 { 4 } else { 2 };

    // Read row counts for each present table, in table-number order.
    let mut row_counts = [0u32; 64];
    let mut p = 24usize;
    let mut present_tables: Vec<u8> = Vec::new();
    for t in 0..64u8 {
        if valid & (1u64 << t) != 0 {
            if p + 4 > stream.len() {
                return Err(Error::InvalidMetadata("row counts truncated".into()));
            }
            let n = u32::from_le_bytes(stream[p..p + 4].try_into().unwrap());
            row_counts[t as usize] = n;
            present_tables.push(t);
            p += 4;
        }
    }
    // Compute table index sizes: 4 bytes if any table has > 65535 rows.
    let mut table_index = [2usize; 64];
    for t in 0..64usize {
        if row_counts[t] > 0xFFFF {
            table_index[t] = 4;
        }
    }

    let index_sizes = IndexSizes {
        string: string_sz,
        guid: guid_sz,
        blob: blob_sz,
        rows: row_counts,
        table_index,
    };

    // Parse each present table's rows.
    let mut rows: Vec<Vec<Row>> = vec![Vec::new(); 64];
    for &t in &present_tables {
        let cols = match schema(t) {
            Some(s) => s,
            None => {
                // Unknown table: we cannot size it, so bail.
                return Err(Error::InvalidMetadata(format!(
                    "no schema for table {t} (present in image)"
                )));
            }
        };
        let row_size: usize = cols.iter().map(|c| index_sizes.col_size(*c)).sum();
        let count = row_counts[t as usize] as usize;
        let mut table_rows = Vec::with_capacity(count);
        for _ in 0..count {
            if p + row_size > stream.len() {
                return Err(Error::InvalidMetadata(format!("table {t} rows truncated")));
            }
            let row_data = &stream[p..p + row_size];
            let mut col_vals = Vec::with_capacity(cols.len());
            let mut q = 0usize;
            for &c in cols {
                let sz = index_sizes.col_size(c);
                let v = match sz {
                    1 => row_data[q] as u32,
                    2 => u16::from_le_bytes([row_data[q], row_data[q + 1]]) as u32,
                    4 => u32::from_le_bytes([row_data[q], row_data[q + 1], row_data[q + 2], row_data[q + 3]]),
                    _ => unreachable!(),
                };
                col_vals.push(v);
                q += sz;
            }
            table_rows.push(Row { cols: col_vals });
            p += row_size;
        }
        rows[t as usize] = table_rows;
    }

    Ok(Tables {
        rows,
        row_counts,
        index_sizes,
    })
}

/// Decode a coded index column value into (table, row). A value of 0 means
/// "no reference" (table = None). The row is stored 1-based in the encoding.
pub fn decode_coded(c: Coded, value: u32) -> CodedIndex {
    if value == 0 {
        return CodedIndex { table: None, row: 0 };
    }
    let tables = c.tables();
    let tag = c.tag_bits();
    let mask = (1u32 << tag) - 1;
    let tag_val = value & mask;
    let row = value >> tag;
    let table = tables.get(tag_val as usize).copied().flatten();
    CodedIndex { table, row }
}

/// Read a blob's first compressed uint (helper used elsewhere).
pub fn blob_uint(blob: &[u8]) -> Result<(usize, usize)> {
    decode_compressed_uint(blob)
}
