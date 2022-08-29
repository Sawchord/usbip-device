//! A collection of functions which allow for better debug output.

use std::fmt::{Debug, Formatter, LowerHex, Result as FmtResult};

/// Just a thin wrapper to allow for printing in hexadecimal
#[derive(Clone)]
pub struct DbgBuf<'a>(pub &'a [u8]);

impl Debug for DbgBuf<'_> {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        if self.0.len() == 0 {
            return f.write_str("[]");
        }

        f.write_str("[ ")?;
        f.write_fmt(format_args!("{:02x}", self))?;
        f.write_str("]")
    }
}

impl LowerHex for DbgBuf<'_> {
    fn fmt(&self, fmt: &mut Formatter) -> FmtResult {
        for byte in self.0 {
            (fmt.write_fmt(format_args!("{:02x} ", byte)))?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct DbgEmpty;

impl Debug for DbgEmpty {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        f.write_str("(empty)")
    }
}

// TODO: Better output of Setup packets
