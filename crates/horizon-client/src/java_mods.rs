use std::path::{Path, PathBuf};

pub const MODS_DIRECTORY: &str = "mods-java";

#[derive(Clone, Debug)]
pub struct JavaModFiles {
    pub game_home: PathBuf,
    pub mods_directory: PathBuf,
    pub jars: Vec<PathBuf>,
    pub java_home: PathBuf,
    pub preparation_root: PathBuf,
}

pub fn mods_directory(game_home: &Path) -> PathBuf {
    game_home.join(MODS_DIRECTORY)
}

#[cfg(feature = "java-mods")]
pub fn extend_dynamic_levels(
    shapes: horizon_client::chunk::LevelShapes,
) -> horizon_client::chunk::LevelShapes {
    if !horizon_jvm::events::enabled() {
        return shapes;
    }
    let levels = match horizon_jvm::events::extra_levels() {
        Ok(levels) => levels,
        Err(error) => {
            log::warn!("Java mods: dynamic level registry could not be read: {error}");
            return shapes;
        }
    };
    merge_dynamic_levels(shapes, levels)
}

#[cfg(feature = "java-mods")]
fn merge_dynamic_levels(
    mut shapes: horizon_client::chunk::LevelShapes,
    levels: Vec<horizon_jvm::events::ExtraLevel>,
) -> horizon_client::chunk::LevelShapes {
    let mut extra = shapes.extra.to_vec();
    let mut added = 0usize;
    let mut dimension_attributes = Vec::new();
    for level in levels {
        if extra.iter().any(|known| known.id == level.id) {
            continue;
        }
        let shape = match horizon_client::chunk::LevelShape::new(
            level.min_y,
            level.height,
            level.logical_height,
            level.has_skylight,
            level.has_ceiling,
        ) {
            Ok(shape) => shape,
            Err(error) => {
                log::warn!(
                    "Java mods: dimension {} has an unsupported build shape: {error}",
                    level.id
                );
                continue;
            }
        };
        if let Some(raw) = level.dimension_type_data.as_ref() {
            let data = java_dimension_type_data(&level, raw);
            let base = horizon_world::dimension_type::DimensionFacts::base_for_type(
                &level.dimension_type,
            );
            match horizon_world::dimension_type::DimensionFacts::patched(
                base,
                &level.dimension_type,
                &data,
            ) {
                Ok(facts) => {
                    if let Ok(key) = horizon_core::LevelKey::parse(&level.id) {
                        horizon_world::dimension_type::DimensionFacts::add_level(key, facts);
                    }
                }
                Err(error) => log::warn!(
                    "Java mods: dimension type {} could not be imported: {error}",
                    level.dimension_type
                ),
            }
            if data
                .get("attributes")
                .is_some_and(|value| {
                    value
                        .as_object()
                        .is_some_and(|attributes| !attributes.is_empty())
                })
                || data.get("timelines").is_some_and(|value| !value.is_null())
            {
                let mut environment = serde_json::Map::new();
                if let Some(attributes) = data.get("attributes") {
                    environment.insert("attributes".into(), attributes.clone());
                }
                if let Some(timelines) = data.get("timelines") {
                    environment.insert("timelines".into(), timelines.clone());
                }
                if let Some(clock) = data.get("default_clock") {
                    environment.insert("default_clock".into(), clock.clone());
                }
                dimension_attributes.push((
                    level.id.clone(),
                    serde_json::Value::Object(environment),
                ));
            }
        }
        extra.push(horizon_client::chunk::ExtraLevel {
            id: level.id.leak(),
            dimension_type: level.dimension_type.leak(),
            shape,
            seed: None,
        });
        added += 1;
    }
    if added > 0 {
        shapes.extra = extra.leak();
        if !dimension_attributes.is_empty() {
            horizon_client::environment::Environment::add_level_attributes(dimension_attributes);
        }
        log::info!(
            "Java mods: imported {added} extra dimension stem(s) from the Java registry snapshot"
        );
    }
    shapes
}

#[cfg(feature = "java-mods")]
fn java_dimension_type_data(
    level: &horizon_jvm::events::ExtraLevel,
    raw: &serde_json::Value,
) -> serde_json::Value {
    let mut data = raw.as_object().cloned().unwrap_or_default();
    data.insert("min_y".into(), serde_json::json!(level.min_y));
    data.insert("height".into(), serde_json::json!(level.height));
    data.insert(
        "logical_height".into(),
        serde_json::json!(level.logical_height),
    );
    data.insert(
        "has_skylight".into(),
        serde_json::json!(level.has_skylight),
    );
    data.insert(
        "has_ceiling".into(),
        serde_json::json!(level.has_ceiling),
    );
    data.insert(
        "coordinate_scale".into(),
        serde_json::json!(level.coordinate_scale),
    );
    let fixed_time = data.get("fixed_time").is_some_and(|value| !value.is_null());
    data.insert("has_fixed_time".into(), serde_json::json!(fixed_time));

    let mut attributes = data
        .get("attributes")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(ultra_warm) = data.get("ultrawarm").and_then(serde_json::Value::as_bool) {
        attributes.insert(
            "minecraft:gameplay/water_evaporates".into(),
            serde_json::json!(ultra_warm),
        );
        attributes.insert(
            "minecraft:gameplay/fast_lava".into(),
            serde_json::json!(ultra_warm),
        );
        attributes.insert(
            "minecraft:gameplay/snow_golem_melts".into(),
            serde_json::json!(ultra_warm),
        );
    }
    if let Some(respawn_anchor_works) = data
        .get("respawn_anchor_works")
        .and_then(serde_json::Value::as_bool)
    {
        attributes.insert(
            "minecraft:gameplay/respawn_anchor_works".into(),
            serde_json::json!(respawn_anchor_works),
        );
    }
    if let Some(piglin_safe) = data.get("piglin_safe").and_then(serde_json::Value::as_bool) {
        attributes.insert(
            "minecraft:gameplay/piglins_zombify".into(),
            serde_json::json!(!piglin_safe),
        );
    }
    if let Some(has_raids) = data.get("has_raids").and_then(serde_json::Value::as_bool) {
        attributes.insert(
            "minecraft:gameplay/can_start_raid".into(),
            serde_json::json!(has_raids),
        );
    }
    if data.get("bed_works").and_then(serde_json::Value::as_bool) == Some(false) {
        attributes.insert(
            "minecraft:gameplay/bed_rule".into(),
            serde_json::json!({
                "can_sleep": "never",
                "can_set_spawn": "never",
                "destroy_on_use": true,
                "destroy_on_leave": false
            }),
        );
        attributes.insert(
            "minecraft:gameplay/straw_bed_rule".into(),
            serde_json::json!({
                "can_sleep": "never",
                "can_set_spawn": "never",
                "destroy_on_use": true,
                "destroy_on_leave": false
            }),
        );
    }
    match data.get("effects").and_then(serde_json::Value::as_str) {
        Some("minecraft:the_nether") => {
            data.insert("skybox".into(), serde_json::json!("none"));
            data.insert("cardinal_light".into(), serde_json::json!("nether"));
            attributes.insert(
                "minecraft:gameplay/sky_light_level".into(),
                serde_json::json!(4.0),
            );
            attributes.insert(
                "minecraft:gameplay/nether_portal_spawns_piglin".into(),
                serde_json::json!(false),
            );
            attributes.insert(
                "minecraft:visual/cloud_color".into(),
                serde_json::Value::Null,
            );
        }
        Some("minecraft:the_end") => {
            data.insert("skybox".into(), serde_json::json!("end"));
            attributes.insert(
                "minecraft:gameplay/nether_portal_spawns_piglin".into(),
                serde_json::json!(false),
            );
            attributes.insert(
                "minecraft:visual/cloud_color".into(),
                serde_json::Value::Null,
            );
        }
        Some(_) => {
            data.entry("skybox".into())
                .or_insert(serde_json::json!("overworld"));
            data.entry("cardinal_light".into())
                .or_insert(serde_json::json!("default"));
        }
        None => {}
    }
    if !attributes.is_empty() {
        data.insert("attributes".into(), serde_json::Value::Object(attributes));
    }
    serde_json::Value::Object(data)
}

#[cfg(not(feature = "java-mods"))]
pub fn extend_dynamic_levels(
    shapes: horizon_client::chunk::LevelShapes,
) -> horizon_client::chunk::LevelShapes {
    shapes
}

pub fn installed_jars(game_home: &Path) -> Vec<PathBuf> {
    #[cfg(feature = "java-mods")]
    {
        horizon_jvm::jars(&mods_directory(game_home))
    }
    #[cfg(not(feature = "java-mods"))]
    {
        let _ = game_home;
        Vec::new()
    }
}

#[cfg(feature = "java-mods")]
pub fn inspect(game_home: &Path) -> Result<Option<JavaModFiles>, String> {
    let mods_directory = mods_directory(game_home);
    let jars = horizon_jvm::jars(&mods_directory);
    if jars.is_empty() {
        return Ok(None);
    }

    let private_home = horizon_client::logging::private_home();
    let java_home = if cfg!(target_os = "android") {
        private_home.join("preparer/jre25")
    } else {
        horizon_jvm::find_java_home()?
    };
    let jvm = horizon_jvm::libjvm(&java_home)?;
    if !jvm.is_file() {
        return Err(format!("Java runtime is missing: {}", jvm.display()));
    }

    let mut preparation_roots = Vec::new();
    if let Some(root) = std::env::var_os("HORIZON_BRIDGE_PREPARATION_ROOT") {
        preparation_roots.push(PathBuf::from(root));
    }
    preparation_roots.push(private_home.join("preparer/kit"));
    preparation_roots.push(game_home.to_path_buf());
    preparation_roots.dedup();

    let mut preparation_root = None;
    let mut preparation_error = None;
    for root in preparation_roots {
        if !root.join("research-lock.json").is_file() {
            continue;
        }
        match (
            horizon_jvm::facade::game_jar(&root),
            horizon_jvm::facade::game_libraries(&root),
        ) {
            (Ok(_), Ok(_)) => {
                preparation_root = Some(root);
                break;
            }
            (Err(error), _) | (_, Err(error)) => preparation_error = Some(error),
        }
    }
    let preparation_root = preparation_root.ok_or_else(|| {
        preparation_error.unwrap_or_else(|| {
            "Minecraft Java metadata and prepared client artifacts are missing from Starling's cache".into()
        })
    })?;

    let lock: serde_json::Value = serde_json::from_slice(
        &std::fs::read(preparation_root.join("research-lock.json"))
            .map_err(|error| format!("research-lock.json: {error}"))?,
    )
    .map_err(|error| format!("research-lock.json: {error}"))?;
    let java_major = lock
        .pointer("/minecraft/java_major")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(horizon_jvm::JAVA_MAJOR);
    if java_major != horizon_jvm::JAVA_MAJOR {
        return Err(format!(
            "Java mods require Java {}, but this Minecraft preparation declares Java {java_major}",
            horizon_jvm::JAVA_MAJOR
        ));
    }

    Ok(Some(JavaModFiles {
        game_home: game_home.to_path_buf(),
        mods_directory,
        jars,
        java_home,
        preparation_root,
    }))
}

#[cfg(not(feature = "java-mods"))]
pub fn inspect(_game_home: &Path) -> Result<Option<JavaModFiles>, String> {
    Ok(None)
}

#[cfg(feature = "java-mods")]
static STARTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Run Horizon's Java registration oracle before Starling freezes its content registries.
/// The exported folder packages use the same native content loader as ordinary Starling mods.
#[cfg(feature = "java-mods")]
pub fn prepare_content(game_home: &Path, resources: &Path) -> Vec<PathBuf> {
    use std::sync::atomic::Ordering;

    let jars = installed_jars(game_home);
    if jars.is_empty() {
        return Vec::new();
    }
    let (loadable, left_out) = horizon_jvm::renderers::split(&jars);
    let content_jars = if left_out.is_empty() { jars } else { loadable };
    if content_jars.is_empty() {
        return Vec::new();
    }
    let cache = horizon_client::logging::cache_dir().join("java-mod-content");
    if let Some(paths) = horizon_jvm::content::cached(&content_jars, &cache) {
        log::info!("Java mods: reused {} generated content package(s)", paths.len());
        return paths;
    }
    if STARTED.swap(true, Ordering::SeqCst) {
        log::warn!("Java mods: registrations changed after the JVM started; restart Starling to rebuild Java content");
        return Vec::new();
    }

    let (content_jars, fabric_loaded) = match start(game_home) {
        Ok(result) => result,
        Err(error) => {
            log::warn!("Java mods: couldn't prepare registry content: {error}");
            return Vec::new();
        }
    };
    if !fabric_loaded || content_jars.is_empty() {
        log::warn!("Java mods: Fabric was not available; custom Java registries are not imported");
        return Vec::new();
    }
    match horizon_jvm::content::build(&content_jars, resources, &cache, &|line| log::info!("{line}")) {
        Ok(paths) => {
            log::info!("Java mods: imported {} Java content package(s) into Starling's native registry pipeline", paths.len());
            paths
        }
        Err(error) => {
            log::warn!("Java mods: Java registry/content import failed: {error}");
            Vec::new()
        }
    }
}

#[cfg(not(feature = "java-mods"))]
pub fn prepare_content(_game_home: &Path, _resources: &Path) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(feature = "java-mods")]
pub fn apply_generated_metadata(prepared_root: &Path) {
    let cache = horizon_client::logging::cache_dir().join("java-mod-content");
    if let Some(source) = horizon_jvm::content::creative(&cache) {
        let target = prepared_root.join(horizon_jvm::content::CREATIVE);
        match merge_creative_inventory(&source, &target) {
            Ok(()) => log::info!("Java mods: merged Java creative tabs into Starling's creative inventory"),
            Err(error) => log::warn!("Java mods: couldn't merge Java creative tabs: {error}"),
        }
    }
    if let Some(path) = horizon_jvm::content::tints(&cache) {
        match std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|json| horizon_client_render::mesher::set_java_tints(&json).map_err(|error| error.to_string()))
        {
            Ok(count) => log::info!("Java mods: loaded tint definitions for {count} block(s)"),
            Err(error) => log::warn!("Java mods: couldn't load Java block tints: {error}"),
        }
    }
}

#[cfg(not(feature = "java-mods"))]
pub fn apply_generated_metadata(_prepared_root: &Path) {}

#[cfg(feature = "java-mods")]
fn merge_creative_inventory(source: &Path, target: &Path) -> Result<(), String> {
    use serde_json::Value;
    let read = |path: &Path| -> Result<Value, String> {
        let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(format!("{} exceeds 16 MiB", path.display()));
        }
        serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
    };
    let mut java = read(source)?;
    let mut native = if target.is_file() { Some(read(target)?) } else { None };
    let Some(java_tabs) = java.get_mut("tabs").and_then(Value::as_array_mut) else {
        return Err("Java creative export has no tabs array".into());
    };
    if let Some(native_tabs) = native.as_mut().and_then(|value| value.get_mut("tabs")).and_then(Value::as_array_mut) {
        for native_tab in native_tabs.iter() {
            let Some(id) = native_tab.get("id").and_then(Value::as_str) else { continue };
            let Some(java_tab) = java_tabs.iter_mut().find(|tab| tab.get("id").and_then(Value::as_str) == Some(id)) else {
                java_tabs.push(native_tab.clone());
                continue;
            };
            merge_tab(java_tab, native_tab);
        }
    }
    if let Some(native) = native.as_ref() {
        if let Some(native_permissions) = native.get("permissions") {
            let permissions = java.get("permissions").and_then(Value::as_bool).unwrap_or(false)
                || native_permissions.as_bool().unwrap_or(false);
            java["permissions"] = Value::Bool(permissions);
        }
    }
    let mut output = Vec::new();
    serde_json::to_writer(&mut output, &java).map_err(|error| error.to_string())?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = target.with_extension("json.java-bridge-tmp");
    std::fs::write(&temporary, output).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, target).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(feature = "java-mods")]
fn merge_tab(java: &mut serde_json::Value, native: &serde_json::Value) {
    use serde_json::Value;
    let search = java.get("type").and_then(Value::as_str) == Some("SEARCH");
    let mut items = java.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut lines = java.get("lines").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut tags = java.get("tags").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut permissions = java.get("permissions_only").and_then(Value::as_array).cloned().unwrap_or_default();
    let native_items = native.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    let native_lines = native.get("lines").and_then(Value::as_array).cloned().unwrap_or_default();
    let native_tags = native.get("tags").and_then(Value::as_array).cloned().unwrap_or_default();
    let native_permissions = native.get("permissions_only").and_then(Value::as_array).cloned().unwrap_or_default();
    for (index, item) in native_items.iter().enumerate() {
        if let Some(existing) = items.iter().position(|value| value == item) {
            if search {
                merge_string_array(&mut lines, existing, native_lines.get(index));
                merge_string_array(&mut tags, existing, native_tags.get(index));
            }
            continue;
        }
        let new_index = items.len();
        items.push(item.clone());
        if search {
            lines.push(native_lines.get(index).cloned().unwrap_or_else(|| Value::Array(Vec::new())));
            tags.push(native_tags.get(index).cloned().unwrap_or_else(|| Value::Array(Vec::new())));
        }
        if native_permissions.iter().any(|value| value.as_u64() == Some(index as u64)) {
            permissions.push(Value::from(new_index as u64));
        }
    }
    java["items"] = Value::Array(items);
    if search {
        java["lines"] = Value::Array(lines);
        java["tags"] = Value::Array(tags);
    }
    if !permissions.is_empty() {
        java["permissions_only"] = Value::Array(permissions);
    }
}

#[cfg(feature = "java-mods")]
fn merge_string_array(target: &mut Vec<serde_json::Value>, index: usize, source: Option<&serde_json::Value>) {
    use serde_json::Value;
    let Some(source) = source.and_then(Value::as_array) else { return };
    while target.len() <= index {
        target.push(Value::Array(Vec::new()));
    }
    let mut merged = target[index].as_array().cloned().unwrap_or_default();
    for value in source {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }
    target[index] = Value::Array(merged);
}

#[cfg(feature = "java-mods")]
static STARTUP_THREAD: std::sync::Mutex<Option<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(None);

#[cfg(feature = "java-mods")]
pub fn start_if_present(game_home: PathBuf) {
    use std::sync::atomic::Ordering;

    if installed_jars(&game_home).is_empty() || STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::lan::packet_tap::undecided();
    let spawned = std::thread::Builder::new()
        .name("Starling Java mods".into())
        .spawn(move || {
            if let Err(error) = start(&game_home) {
                crate::lan::packet_tap::want(false);
                log::warn!("Java mods: startup failed: {error}");
            }
        });
    match spawned {
        Ok(thread) => {
            *STARTUP_THREAD.lock().unwrap_or_else(|e| e.into_inner()) = Some(thread);
        }
        Err(error) => {
            STARTED.store(false, Ordering::SeqCst);
            crate::lan::packet_tap::want(false);
            log::warn!("Java mods: couldn't start the runtime thread: {error}");
        }
    }
}

#[cfg(feature = "java-mods")]
pub(super) fn wait_for_startup() {
    if let Some(thread) = STARTUP_THREAD
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        && thread.join().is_err()
    {
        log::warn!("Java mods: runtime startup thread panicked");
    }
}

#[cfg(not(feature = "java-mods"))]
pub(super) fn wait_for_startup() {}

pub(crate) use crate::java_mods_worldgen::worldgen_terrain;

#[cfg(feature = "java-mods")]
pub fn shutdown() {
    crate::lan::packet_tap::want(false);
    if let Some(thread) = STARTUP_THREAD
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        && thread.join().is_err()
    {
        log::warn!("Java mods: runtime startup thread panicked");
    }
    horizon_jvm::exit(
        0,
        std::time::Duration::from_secs(90),
        horizon_client::logging::flush,
    );
}

#[cfg(feature = "java-mods")]
fn start(game_home: &Path) -> Result<(Vec<PathBuf>, bool), String> {
    let Some(files) = inspect(game_home)? else {
        return Ok((Vec::new(), false));
    };
    let started = std::time::Instant::now();
    let work = horizon_client::logging::private_home().join(".local/bridge");
    std::fs::create_dir_all(&work).map_err(|error| format!("{}: {error}", work.display()))?;
    #[cfg(target_os = "android")]
    supply_android_java(&files.java_home)?;

    let has_fabric_api = files.jars.iter().any(|jar| {
        jar.file_name().is_some_and(|name| {
            name.to_string_lossy()
                .to_ascii_lowercase()
                .starts_with("fabric-api")
        })
    });
    let mut add_mods = if has_fabric_api {
        Vec::new()
    } else {
        vec![horizon_jvm::libs::ensure_fabric_api(&work.join("fabric-api"))?]
    };

    let (loadable, left_out) = horizon_jvm::renderers::split(&files.jars);
    let content_jars = if left_out.is_empty() { files.jars.clone() } else { loadable.clone() };
    let mods = if left_out.is_empty() {
        files.mods_directory.clone()
    } else {
        add_mods.extend(loadable.iter().cloned());
        let empty = work.join("no-mods");
        if empty.exists() {
            std::fs::remove_dir_all(&empty)
                .map_err(|error| format!("{}: {error}", empty.display()))?;
        }
        std::fs::create_dir_all(&empty)
            .map_err(|error| format!("{}: {error}", empty.display()))?;
        empty
    };

    let lock = std::fs::read(files.preparation_root.join("research-lock.json"))
        .map_err(|error| format!("research-lock.json: {error}"))?;
    let lock: serde_json::Value =
        serde_json::from_slice(&lock).map_err(|error| format!("research-lock.json: {error}"))?;
    let version = lock
        .pointer("/minecraft/version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "research-lock.json has no minecraft version".to_owned())?
        .to_owned();

    let config = horizon_jvm::Config {
        home: files.java_home.clone(),
        work: work.clone(),
        jars: files.jars.clone(),
        class_path: Vec::new(),
        fabric: Some(horizon_jvm::Fabric {
            game_jar: horizon_jvm::facade::game_jar(&files.preparation_root)?,
            libraries: horizon_jvm::facade::game_libraries(&files.preparation_root)?,
            add_mods,
            platform: horizon_jvm::has_platform(),
            mods,
            game_dir: files.game_home.clone(),
            version,
            side: horizon_jvm::Side::Client,
        }),
        class_archive: None,
    };
    let bridge = horizon_jvm::start(&config)?;
    crate::lan::packet_tap::want(horizon_jvm::events::wants_packets());
    log::info!(
        "Java mods: JVM {} from {} and Fabric started in {} ms; {} jar(s)",
        bridge.java_version(),
        bridge.home().display(),
        started.elapsed().as_millis(),
        files.jars.len()
    );
    let fabric_loaded = bridge.fabric();
    if fabric_loaded {
        log::info!("Java mods: Fabric loaded {}", bridge.mods().join(", "));
    }
    let skipped: Vec<String> = left_out.iter().map(|item| item.line()).collect();
    for line in &skipped {
        log::warn!("Java mods: {line}");
    }
    if !skipped.is_empty() {
        if let Err(error) = horizon_jvm::events::not_needed(&skipped) {
            log::warn!("Java mods: unsupported renderer mods weren't reported: {error}");
        }
    }
    for line in horizon_jvm::renderers::partial_lines(bridge.mods()) {
        log::warn!("Java mods: {line}");
    }
    Ok((content_jars, fabric_loaded))
}

#[cfg(target_os = "android")]
fn supply_android_java(java_home: &Path) -> Result<(), String> {
    let library = crate::platform::library_dir()
        .ok_or("Starling's Android Java launcher libraries are unavailable")?;
    let java = library.join("libstarling_java.so");
    let javac = library.join("libstarling_javac.so");
    for tool in [&java, &javac] {
        if !tool.is_file() {
            return Err(format!("Java tool is missing: {}", tool.display()));
        }
    }
    let temp = horizon_client::logging::private_home().join("preparer/tmp");
    std::fs::create_dir_all(&temp).map_err(|error| format!("{}: {error}", temp.display()))?;
    let home = java_home.display().to_string();
    let library = library.display().to_string();
    let temp = temp.display().to_string();
    let _ = horizon_prepare::java::supply(horizon_prepare::java::Supplied {
        java,
        javac,
        env: vec![
            ("STARLING_JAVA_HOME".into(), home.clone()),
            ("LD_LIBRARY_PATH".into(), format!("{home}/lib:{library}")),
            ("JAVA_HOME".into(), home),
            ("TMPDIR".into(), temp.clone()),
            ("JAVA_TOOL_OPTIONS".into(), format!("-Djava.io.tmpdir={temp}")),
        ],
    });
    Ok(())
}

#[cfg(all(test, feature = "java-mods"))]
mod dynamic_level_tests {
    use super::merge_dynamic_levels;
    use horizon_client::chunk::{ExtraLevel, LevelShapes};
    use horizon_jvm::events::ExtraLevel as JavaExtraLevel;

    fn java_level(
        id: &str,
        dimension_type: &str,
        min_y: i32,
        height: i32,
        logical_height: i32,
    ) -> JavaExtraLevel {
        JavaExtraLevel {
            id: id.to_owned(),
            dimension_type: dimension_type.to_owned(),
            min_y,
            height,
            logical_height,
            has_skylight: true,
            has_ceiling: false,
            coordinate_scale: 1.0,
            dimension_type_data: None,
        }
    }

    fn java_level_with_data(
        id: &str,
        dimension_type: &str,
        data: serde_json::Value,
    ) -> JavaExtraLevel {
        let mut level = java_level(id, dimension_type, -32, 256, 240);
        level.has_skylight = false;
        level.has_ceiling = true;
        level.coordinate_scale = 2.0;
        level.dimension_type_data = Some(data);
        level
    }

    #[test]
    fn imports_java_dimension_type_gameplay_and_environment_fields() {
        let level_id = "mod:thermal_test";
        let level = java_level_with_data(
            level_id,
            "mod:thermal_type",
            serde_json::json!({
                "fixed_time": 6000,
                "infiniburn": "#mod:infiniburn_thermal",
                "ambient_light": 0.2,
                "monster_spawn_light_level": {"type": "minecraft:constant", "value": 7},
                "monster_spawn_block_light_limit": 13,
                "ultrawarm": true,
                "respawn_anchor_works": true,
                "piglin_safe": true,
                "has_raids": false,
                "bed_works": false,
                "effects": "minecraft:the_nether",
                "timelines": ["mod:thermal_day"],
                "default_clock": "mod:thermal_clock"
            }),
        );
        let data = java_dimension_type_data(&level, level.dimension_type_data.as_ref().unwrap());
        let facts = horizon_world::dimension_type::DimensionFacts::patched(
            horizon_world::dimension_type::DimensionFacts::base_for_type(
                &level.dimension_type,
            ),
            &level.dimension_type,
            &data,
        )
        .unwrap();
        assert_eq!(facts.id, "mod:thermal_type");
        assert!(facts.has_fixed_time);
        assert_eq!(facts.fixed_time, Some(6_000));
        assert_eq!(facts.coordinate_scale, 2.0);
        assert_eq!(facts.height.min_y, -32);
        assert_eq!(facts.height.logical_height, 240);
        assert!(!facts.height.has_skylight);
        assert!(facts.height.has_ceiling);
        assert_eq!(facts.infiniburn, "#mod:infiniburn_thermal");
        assert_eq!(facts.ambient_light, 0.2);
        assert_eq!(facts.monster_spawn_light_level.bounds(), [7, 7]);
        assert_eq!(facts.monster_spawn_block_light_limit, 13);
        assert!(facts.water_evaporates);
        assert!(facts.fast_lava);
        assert!(facts.respawn_anchor_works);
        assert!(!facts.piglins_zombify);
        assert!(!facts.can_start_raid);
        assert_eq!(
            facts.bed_rule.can_sleep,
            horizon_world::dimension_type::BedRuleTest::Never
        );
        assert_eq!(
            facts.straw_bed_rule.can_set_spawn,
            horizon_world::dimension_type::BedRuleTest::Never
        );
        assert!(facts.snow_golem_melts);
        assert!(!facts.nether_portal_spawns_piglin);
        assert_eq!(facts.timelines, vec!["mod:thermal_day".to_owned()]);
        assert_eq!(facts.default_clock.as_deref(), Some("mod:thermal_clock"));
        assert_eq!(facts.skybox, horizon_world::dimension_type::Skybox::None);
        assert_eq!(facts.cardinal_light, horizon_world::dimension_type::CardinalLight::Nether);
    }

    #[test]
    fn imports_extra_level_shape_and_skips_duplicate_ids() {
        let result = merge_dynamic_levels(
            LevelShapes::VANILLA,
            vec![
                java_level("mod:moon", "mod:moon_type", -32, 256, 256),
                java_level("mod:moon", "mod:moon_type", -32, 256, 256),
            ],
        );
        assert_eq!(result.extra.len(), 1);
        assert_eq!(result.extra[0].id, "mod:moon");
        assert_eq!(result.extra[0].dimension_type, "mod:moon_type");
        assert_eq!(result.extra[0].shape.min_y, -32);
        assert_eq!(result.extra[0].shape.height, 256);
    }

    #[test]
    fn refuses_invalid_extra_level_shapes() {
        let result = merge_dynamic_levels(
            LevelShapes::VANILLA,
            vec![java_level("mod:bad", "mod:bad_type", 3, 250, 300)],
        );
        assert!(result.extra.is_empty());
    }

    #[test]
    fn native_level_metadata_wins_over_java_duplicate() {
        let native_id = "mod:native".to_owned().leak();
        let native_type = "mod:native_type".to_owned().leak();
        let shape = horizon_client::chunk::LevelShape::new(-64, 384, 384, true, false).unwrap();
        let native = ExtraLevel {
            id: native_id,
            dimension_type: native_type,
            shape,
            seed: Some(42),
        };
        let result = merge_dynamic_levels(
            LevelShapes { extra: Box::leak(vec![native].into_boxed_slice()), ..LevelShapes::VANILLA },
            vec![java_level("mod:native", "mod:override", 0, 128, 128)],
        );
        assert_eq!(result.extra.len(), 1);
        assert_eq!(result.extra[0].seed, Some(42));
        assert_eq!(result.extra[0].dimension_type, "mod:native_type");
    }
}
