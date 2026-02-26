use std::{fmt, str::FromStr};

use roxmltree::Document;

use crate::http::{
    ParseError, QueryBuilder, QueryBuilderError, QueryIter, QueryParam, Request, TextResponse,
    helper::parse_xml_root_node, pair::parse_xml_child_paired,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PairPhase5Request {
    pub device_name: String,
}

impl Request for PairPhase5Request {
    fn append_query_params(
        &self,
        query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        query_builder.append(QueryParam {
            key: "phrase",
            value: "pairchallenge",
        });
        query_builder.append(QueryParam {
            key: "devicename",
            value: &self.device_name,
        });
        query_builder.append(QueryParam {
            key: "updateState",
            value: "1",
        });

        Ok(())
    }

    fn from_query_params<'a, Q>(query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        todo!()
    }
}

pub struct PairPhase5Response {
    pub paired: bool,
}

impl TextResponse for PairPhase5Response {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        todo!()
    }
}

impl FromStr for PairPhase5Response {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let doc = Document::parse(s)?;
        let root = parse_xml_root_node(&doc)?;

        let paired = parse_xml_child_paired(root)?;

        Ok(Self { paired })
    }
}
