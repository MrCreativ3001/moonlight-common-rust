use crate::http::{QueryBuilder, QueryBuilderError, QueryIter, QueryParam, Request};

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

    fn from_query_params<'a, Q>(query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        todo!()
    }
}
