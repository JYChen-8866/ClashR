pub mod connections;
pub mod home;
pub mod profiles;
pub mod proxies;
pub mod settings;

pub use connections::ConnectionsPage;
pub use home::HomePage;
pub use profiles::ProfilesPage;
pub use proxies::ProxiesPage;
pub use settings::SettingsPage;

pub struct RulesPage;
pub struct LogsPage;
