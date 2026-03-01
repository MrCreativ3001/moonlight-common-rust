use std::fmt;
use std::fmt::Write as _;

use roxmltree::{Document, Node};

use crate::http::ParseError;

pub fn parse_xml_child_text<'doc, 'node>(
    list_node: Node<'node, 'doc>,
    name: &'static str,
) -> Result<&'node str, ParseError>
where
    'node: 'doc,
{
    let node = list_node
        .children()
        .find(|node| node.tag_name().name() == name)
        .ok_or(ParseError::DetailNotFound(name))?;
    let content = node.text().ok_or(ParseError::XmlTextNotFound(name))?;

    Ok(content)
}

pub fn parse_xml_root_node<'doc>(doc: &'doc Document) -> Result<Node<'doc, 'doc>, ParseError> {
    let root = doc
        .root()
        .children()
        .find(|node| node.tag_name().name() == "root")
        .ok_or(ParseError::XmlRootNotFound)?;

    // Important: status code can be negative
    let status_code = root
        .attribute("status_code")
        .ok_or(ParseError::DetailNotFound("status_code"))?
        .parse::<i32>()?;

    if status_code / 100 != 2 {
        return Err(ParseError::InvalidXmlStatusCode {
            message: root.attribute("status_message").map(str::to_string),
        });
    }

    Ok(root)
}

pub fn serialize_text_xml(writer: &mut impl fmt::Write, input: &str) -> std::fmt::Result {
    for c in input.chars() {
        match c {
            '&' => writer.write_str("&amp;")?,
            '<' => writer.write_str("&lt;")?,
            '>' => writer.write_str("&gt;")?,
            '"' => writer.write_str("&quot;")?,
            '\'' => writer.write_str("&apos;")?,
            _ => writer.write_char(c)?,
        }
    }
    Ok(())
}

pub struct CounterWriter<'a> {
    buf: &'a mut [u8],
    pos: usize, // tracks how many bytes have been written
}

impl<'a> fmt::Write for CounterWriter<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        if self.pos + bytes.len() > self.buf.len() {
            return Err(fmt::Error); // buffer overflow
        }
        self.buf[self.pos..self.pos + bytes.len()].copy_from_slice(bytes);
        self.pos += bytes.len();
        Ok(())
    }
}

pub fn u32_to_str(num: u32, buffer: &mut [u8; 11]) -> &str {
    fmt_write_to_buffer(buffer, |writer| write!(writer, "{num}").expect("write u32"))
}
pub fn i32_to_str(num: i32, buffer: &mut [u8; 11]) -> &str {
    fmt_write_to_buffer(buffer, |writer| write!(writer, "{num}").expect("write i32"))
}
pub fn fmt_write_to_buffer(buffer: &mut [u8], fmt: impl FnOnce(&mut CounterWriter)) -> &str {
    let mut writer = CounterWriter {
        buf: buffer,
        pos: 0,
    };

    fmt(&mut writer);

    let pos = writer.pos;

    str::from_utf8(&buffer[0..pos]).expect("valid utf8 bytes")
}
