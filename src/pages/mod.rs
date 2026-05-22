pub mod home;
pub mod profiles;
pub mod proxies;
pub mod settings;

pub use home::HomePage;
pub use profiles::ProfilesPage;
pub use proxies::ProxiesPage;
pub use settings::SettingsPage;

pub struct ConnectionsPage;
pub struct RulesPage;
pub struct LogsPage;
