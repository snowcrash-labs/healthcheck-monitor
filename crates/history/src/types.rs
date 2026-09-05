//! Validated SQL values keep database constraints enforceable before insertion and after decoding.
use diesel::{
    deserialize::{FromSql, Queryable},
    expression::AsExpression,
    pg::{Pg, PgValue},
    serialize::{IsNull, Output, ToSql},
    sql_types::{Text, Uuid as SqlUuid},
};
use nutype::nutype;
use std::io::Write;

macro_rules! text_type {
    ($name:ident, $predicate:expr) => {
        #[nutype(validate(predicate = $predicate), derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Serialize, Deserialize))]
        pub struct $name(String);
        impl AsExpression<Text> for $name {
            type Expression = diesel::dsl::AsExprOf<String, Text>;
            fn as_expression(self) -> Self::Expression { <String as AsExpression<Text>>::as_expression(self.into_inner()) }
        }
        impl<'a> AsExpression<Text> for &'a $name {
            type Expression = diesel::dsl::AsExprOf<&'a str, Text>;
            fn as_expression(self) -> Self::Expression { let value: &str = self.as_ref(); <&str as AsExpression<Text>>::as_expression(value) }
        }
        impl ToSql<Text, Pg> for $name {
            fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> diesel::serialize::Result {
                let value: &str = self.as_ref(); out.write_all(value.as_bytes())?; Ok(IsNull::No)
            }
        }
        impl FromSql<Text, Pg> for $name {
            fn from_sql(value: PgValue<'_>) -> diesel::deserialize::Result<Self> {
                Self::try_new(<String as FromSql<Text, Pg>>::from_sql(value)?).map_err(|_| std::io::Error::other("invalid bounded database text").into())
            }
        }
        impl Queryable<Text, Pg> for $name {
            type Row = String;
            fn build(value: String) -> diesel::deserialize::Result<Self> {
                Self::try_new(value).map_err(|_| std::io::Error::other("invalid bounded database text").into())
            }
        }
    };
}
text_type!(Digest, |value: &str| value.len() == 64
    && value.bytes().all(
        |byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
    ));
text_type!(Name, |value: &str| !value.is_empty()
    && value.chars().count() <= 128
    && !value.contains('\0'));
text_type!(Resource, |value: &str| !value.is_empty()
    && value.chars().count() <= 2048
    && !value.contains('\0'));
text_type!(Evidence, |value: &str| !value.is_empty()
    && value.chars().count() <= 1024
    && !value.contains('\0'));

#[nutype(validate(predicate = |value: &uuid::Uuid| value.get_version_num() == 7 && value.get_variant() == uuid::Variant::RFC4122), derive(Debug, Clone, Copy, PartialEq, Eq, Hash, AsRef, Serialize, Deserialize))]
pub struct Id(uuid::Uuid);
impl AsExpression<SqlUuid> for Id {
    type Expression = diesel::dsl::AsExprOf<uuid::Uuid, SqlUuid>;
    fn as_expression(self) -> Self::Expression {
        <uuid::Uuid as AsExpression<SqlUuid>>::as_expression(self.into_inner())
    }
}
impl<'a> AsExpression<SqlUuid> for &'a Id {
    type Expression = diesel::dsl::AsExprOf<&'a uuid::Uuid, SqlUuid>;
    fn as_expression(self) -> Self::Expression {
        <&uuid::Uuid as AsExpression<SqlUuid>>::as_expression(self.as_ref())
    }
}
impl ToSql<SqlUuid, Pg> for Id {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> diesel::serialize::Result {
        <uuid::Uuid as ToSql<SqlUuid, Pg>>::to_sql(self.as_ref(), out)
    }
}
impl FromSql<SqlUuid, Pg> for Id {
    fn from_sql(value: PgValue<'_>) -> diesel::deserialize::Result<Self> {
        Self::try_new(<uuid::Uuid as FromSql<SqlUuid, Pg>>::from_sql(value)?)
            .map_err(|_| std::io::Error::other("database identity is not UUIDv7").into())
    }
}
impl Queryable<SqlUuid, Pg> for Id {
    type Row = uuid::Uuid;
    fn build(value: uuid::Uuid) -> diesel::deserialize::Result<Self> {
        Self::try_new(value)
            .map_err(|_| std::io::Error::other("database identity is not UUIDv7").into())
    }
}
