use crate::http::{
    Endpoint, QueryBuilder, QueryBuilderError, QueryIter, QueryParam, Request, helper::u32_to_str,
};

pub struct AppBoxArtEndpoint;

impl Endpoint for AppBoxArtEndpoint {
    type Request = AppBoxArtRequest;
    type Response = Vec<u8>;

    fn path() -> &'static str {
        "/appasset"
    }

    fn https_required() -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppBoxArtRequest {
    pub app_id: u32,
}

impl Request for AppBoxArtRequest {
    fn append_query_params(
        &self,
        query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        let mut appid_buffer = [0u8; _];
        let appid = u32_to_str(self.app_id, &mut appid_buffer);
        query_builder.append(QueryParam {
            key: "appid",
            value: appid,
        })?;

        query_builder.append(QueryParam {
            key: "AssetType",
            value: "2",
        })?;
        query_builder.append(QueryParam {
            key: "AssetIdx",
            value: "0",
        })?;

        Ok(())
    }

    fn from_query_params<'a, Q>(query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        todo!()
    }
}
