//! The pin's system/user configuration driver, including its fixed paths.

use oxpinyin_data::table_info::{FileName, FileType, TableConf, TableConfError, TableRow};
use oxpinyin_data::user_files::{SystemVersions, UserTableInfo};
use oxpinyin_input::tool;
use oxpinyin_input::tool::table_info as output;

fn name(value: Option<&FileName>) -> Option<std::ffi::CString> {
    value.and_then(|value| oxpinyin_input::c_path(std::path::Path::new(value.as_ref())))
}

fn dump(row: &TableRow) {
    match row.file_type {
        FileType::NotUsed => output::not_used(),
        FileType::SystemFile => output::system_file(
            name(row.table.as_ref()).as_deref(),
            name(row.system.as_ref()).as_deref(),
            name(row.user.as_ref()).as_deref(),
        ),
        FileType::Dictionary => output::dictionary(
            name(row.table.as_ref()).as_deref(),
            name(row.system.as_ref()).as_deref(),
        ),
        FileType::UserFile => output::user_file(name(row.user.as_ref()).as_deref()),
    }
}

/// `tests/storage/test_table_info.cpp:51-99` at 074a2219.
pub fn main() {
    let _ = tool::set_locale_from_environment();
    let system = match TableConf::load(c"../../data/table.conf") {
        Ok(conf) => conf,
        Err(TableConfError::Abort(_) | TableConfError::UninitialisedSourceFormat) => tool::abort(),
        Err(TableConfError::Unopenable | TableConfError::Header) => {
            tool::storage::report_table_conf_failed();
            tool::exit(tool::ENOENT);
        }
    };
    output::lambda(system.lambda_f32());
    for addon in [false, true] {
        for index in 0..16_u8 {
            // TableConf retains a fixed slot per library, so the pin's
            // m_dict_index assertions are invariants of the indexed storage.
            let row = if addon {
                system.addon_row(index)
            } else {
                system.default_row(index)
            };
            if let Some(row) = row {
                output::index(addon, index);
                dump(row);
            }
        }
    }
    let versions = SystemVersions::from_conf(&system);
    let fresh = UserTableInfo {
        binary_format_version: 0,
        model_data_version: 0,
        database_format: None,
        open_counter: 0,
    };
    // Maintainer class (b) ruling: a fresh format is None, including when
    // both versions are zero (pin table_info.cpp:313-317,399-406).
    if fresh.is_conform(&versions) {
        tool::abort();
    }
    let user = UserTableInfo::conform_to(&versions);
    if !user.is_conform(&versions) {
        tool::abort();
    }
    let conf = crate::fixed_tmp::path("user.conf");
    if !user.save(&conf) {
        tool::abort();
    }
    let user = UserTableInfo::load(&conf).unwrap_or_else(|_| tool::abort());
    if !user.is_conform(&versions) {
        tool::abort();
    }
}
