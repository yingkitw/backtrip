pub mod decoder;
pub mod disasm;
pub mod opcodes;

pub use decoder::{decode, Instruction, Operand};
