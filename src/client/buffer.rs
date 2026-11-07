use super::Client;
use crate::protocol::{Buffer, BufferRequest, PasteBufferOpts, Request, Response};

impl Client {
    /// Pushes a paste buffer; `name` replaces the buffer of that name, `append` adds to it instead.
    pub async fn set_buffer(
        &mut self,
        name: Option<&str>,
        data: &str,
        append: bool,
    ) -> Result<(), String> {
        let req = Request::Buffer(BufferRequest::Set {
            name: name.map(str::to_string),
            data: data.to_string(),
            append,
        });
        self.call_ok("SetBuffer", req).await
    }

    /// The named paste buffer, or the most recent one.
    pub async fn get_buffer(&mut self, name: Option<&str>) -> Result<Buffer, String> {
        let req = Request::Buffer(BufferRequest::Get {
            name: name.map(str::to_string),
        });
        request!(self, "GetBuffer", req, Response::Buffer(buffer) => buffer)
    }

    /// Paste buffers, newest first.
    pub async fn list_buffers(&mut self) -> Result<Vec<Buffer>, String> {
        request!(self, "ListBuffers", Request::Buffer(BufferRequest::List), Response::Buffers(buffers) => buffers)
    }

    pub async fn delete_buffer(&mut self, name: Option<&str>) -> Result<(), String> {
        let req = Request::Buffer(BufferRequest::Delete {
            name: name.map(str::to_string),
        });
        self.call_ok("DeleteBuffer", req).await
    }

    /// Renames a buffer (`set-buffer -n`); `name` None is the newest buffer.
    pub async fn rename_buffer(&mut self, name: Option<&str>, to: &str) -> Result<(), String> {
        let req = Request::Buffer(BufferRequest::Rename {
            name: name.map(str::to_string),
            to: to.to_string(),
        });
        self.call_ok("RenameBuffer", req).await
    }

    pub async fn paste_buffer(&mut self, opts: PasteBufferOpts) -> Result<(), String> {
        let req = Request::Buffer(BufferRequest::Paste(opts));
        self.call_ok("PasteBuffer", req).await
    }
}
