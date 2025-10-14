use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;

pub static CUR_LEADER: LazyLock<Arc<RwLock<Option<String>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));
