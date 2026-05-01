use std::path::PathBuf;

use crate::groundcover::{DELETED_PLUGIN_NAME, GROUNDCOVER_PLUGIN_NAME};

#[must_use]
pub fn output_directory() -> PathBuf {
    PathBuf::from(".")
}

#[must_use]
pub fn groundcover_output() -> String {
    GROUNDCOVER_PLUGIN_NAME.to_owned()
}

#[must_use]
pub fn deleted_output() -> String {
    DELETED_PLUGIN_NAME.to_owned()
}

#[must_use]
pub fn grass_ids() -> Vec<String> {
    vec![
        "grass".into(),
        "kelp".into(),
        "lilypad".into(),
        "fern".into(),
        "thirrlily".into(),
        "spartium".into(),
        "in_cave_plant".into(),
        "reedgroup".into(),
        "t_mw_floratv_treezifa".into(),
        "t_mw_florash_bush".into(),
        "t_mw_floraow_varga".into(),
        "t_cyr_floragc_shrub".into(),
        "t_cyr_floragc_bush_02".into(),
        "t_glb_flora_cattails".into(),
        "t_cyr_florastr_shrub".into(),
        "flora_bm_shrub".into(),
    ]
}

#[must_use]
pub fn exclude() -> Vec<String> {
    vec![
        "refernce".into(),
        "infernace".into(),
        "planter".into(),
        "_furn_".into(),
        "_skelp".into(),
        "t_glb_var_skeleton".into(),
        "cliffgrass".into(),
        "terr".into(),
        "grassplane".into(),
        "flora_s_m_10_grass".into(),
        "cave_mud_rocks_fern".into(),
        "ab_in_cavemold".into(),
        "rp_mh_rock".into(),
        "ex_cave_grass00".into(),
        "secret_fern".into(),
        "flora_grass_entrance".into(),
    ]
}

#[must_use]
pub fn ignored_plugins() -> Vec<String> {
    vec![
        DELETED_PLUGIN_NAME.into(),
        // Historical config/comments disagree on underscore vs hyphen. Ignore both, because
        // circular generated masters are not improved by winning a spelling argument.
        "deleted-groundcover.omwaddon".into(),
    ]
}
