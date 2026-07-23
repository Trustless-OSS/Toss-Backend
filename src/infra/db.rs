use diesel_async::pooled_connection::bb8::Pool;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::AsyncPgConnection;

/// bb8 connection pool over diesel-async Postgres connections.
pub type DbPool = Pool<AsyncPgConnection>;

/// A connection checked out from [`DbPool`].
pub type DbConn<'a> = diesel_async::pooled_connection::bb8::PooledConnection<'a, AsyncPgConnection>;

/// Build a lazy pool: connections are established on first use rather than up
/// front, mirroring the previous `connect_lazy` behavior.
pub fn connect_lazy(database_url: &str) -> DbPool {
    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(database_url);
    Pool::builder().build_unchecked(manager)
}
