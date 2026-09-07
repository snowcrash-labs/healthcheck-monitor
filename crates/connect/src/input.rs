//! Bound MCP frames before the SDK's line-oriented decoder accumulates them.
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, ReadBuf};
pub struct Limited<R> {
    inner: R,
    length: usize,
}
impl<R> Limited<R> {
    pub fn new(inner: R) -> Self {
        Self { inner, length: 0 }
    }
}
impl<R: AsyncRead + Unpin> AsyncRead for Limited<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                for byte in &buf.filled()[before..] {
                    this.length += 1;
                    if this.length > 65536 {
                        buf.set_filled(before);
                        return Poll::Ready(Err(std::io::Error::other("MCP frame limit")));
                    }
                    if *byte == b'\n' {
                        this.length = 0;
                    }
                }
                Poll::Ready(Ok(()))
            }
            result => result,
        }
    }
}
