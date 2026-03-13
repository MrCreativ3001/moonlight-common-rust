use crate::http::{FromQueryError, QueryBuilder, QueryBuilderError, QueryMap, QueryParam, Request};

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
        })?;
        query_builder.append(QueryParam {
            key: "devicename",
            value: &self.device_name,
        })?;
        query_builder.append(QueryParam {
            key: "updateState",
            value: "1",
        })?;

        Ok(())
    }

    fn from_query_params<Q>(query_map: &Q) -> Result<Self, FromQueryError>
    where
        Q: QueryMap,
    {
        let device_name = query_map.get("devicename")?;

        // TODO: check update_state?
        // let update_state: i32 = query_map.get("updateState")?.parse()?;

        Ok(Self {
            device_name: device_name.into_owned(),
        })
    }
}
