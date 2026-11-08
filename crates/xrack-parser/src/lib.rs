//! ANTLR4 C++ frontend behind an ownership-safe C ABI.
use ast::{Diagnostic, Document, Span};
use serde::Deserialize;
pub use xrack_syntax::ast;

#[repr(C)]
struct Buffer {
    data: *mut u8,
    len: usize,
}
unsafe extern "C" {
    fn xrack_parse(source: *const u8, len: usize) -> Buffer;
    fn xrack_parse_free(buffer: Buffer);
}
struct OwnedBuffer(Buffer);
impl Drop for OwnedBuffer {
    fn drop(&mut self) {
        // SAFETY: this buffer has exactly one owner and is released by its allocator.
        unsafe {
            xrack_parse_free(Buffer {
                data: self.0.data,
                len: self.0.len,
            });
        }
    }
}
#[derive(Deserialize)]
struct Response {
    document: Option<Document>,
    diagnostics: Vec<Diagnostic>,
}

pub fn parse(source: &str) -> Result<Document, Vec<Diagnostic>> {
    // SAFETY: C++ borrows source only during the call, owns the returned allocation,
    // and catches exceptions at the ABI boundary.
    let buffer = OwnedBuffer(unsafe { xrack_parse(source.as_ptr(), source.len()) });
    let error = |message: String| {
        vec![Diagnostic {
            span: Span { line: 1, column: 1 },
            message,
        }]
    };
    if buffer.0.data.is_null() {
        return Err(error("native parser allocation failed".into()));
    }
    // SAFETY: C++ returns `len` initialized bytes that outlive deserialization.
    let bytes = unsafe { std::slice::from_raw_parts(buffer.0.data, buffer.0.len) };
    let response: Response = serde_json::from_slice(bytes)
        .map_err(|e| error(format!("invalid parser response: {e}")))?;
    if !response.diagnostics.is_empty() {
        return Err(response.diagnostics);
    }
    response
        .document
        .ok_or_else(|| error("native parser returned no document".into()))
}
