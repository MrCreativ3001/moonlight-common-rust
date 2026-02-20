use std::{fmt, str::FromStr};

use roxmltree::Document;

use crate::http::{
    Endpoint, ParseError, QueryBuilder, QueryBuilderError, QueryIter, Request, TextResponse,
    helper::{parse_xml_child_text, parse_xml_root_node},
};

pub struct AppListEndpoint;

impl Endpoint for AppListEndpoint {
    type Request = AppListRequest;
    type Response = AppListResponse;

    fn path() -> &'static str {
        "/applist"
    }

    fn https_required() -> bool {
        true
    }
}

#[derive(Debug, PartialEq)]
pub struct AppListRequest {}

impl Request for AppListRequest {
    fn append_query_params(
        &self,
        _query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        Ok(())
    }
    fn from_query_params<'a, Q>(_query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        Ok(AppListRequest {})
    }
}

// TODO: move App somewhere else
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub id: u32,
    pub title: String,
    pub is_hdr_supported: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppListResponse {
    pub apps: Vec<App>,
}

impl TextResponse for AppListResponse {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        todo!()
    }
}

impl FromStr for AppListResponse {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let doc = Document::parse(s.as_ref())?;
        let root = parse_xml_root_node(&doc)?;

        let mut apps = Vec::new();

        for app_node in root
            .children()
            .filter(|node| node.tag_name().name() == "App")
        {
            let title = parse_xml_child_text(app_node, "AppTitle")?.to_string();

            let id = parse_xml_child_text(app_node, "ID")?.parse()?;

            let is_hdr_supported = parse_xml_child_text(app_node, "IsHdrSupported")
                .unwrap_or("0")
                .parse::<u32>()?
                == 1;

            let app = App {
                id,
                title,
                is_hdr_supported,
            };
            apps.push(app);
        }

        Ok(AppListResponse { apps })
    }
}
