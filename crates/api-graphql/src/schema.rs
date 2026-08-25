use crate::context::GraphQLContext;
use crate::query::Query;
use async_graphql::{EmptyMutation, EmptySubscription, Schema};

pub type GogentoSchema = Schema<Query, EmptyMutation, EmptySubscription>;

pub fn build_schema(context: GraphQLContext) -> GogentoSchema {
    Schema::build(Query, EmptyMutation, EmptySubscription).data(context).finish()
}
