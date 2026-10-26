use std::path::Path;

pub fn export_type<T: ts_rs::TS + 'static>(label: &str) {
    let mut delays = [10, 20, 40, 80, 160, 320, 640].into_iter();
    loop {
        match T::export_all() {
            Ok(()) => return,
            // Windows 文件监听器可能短暂持有生成文件，仅对共享锁冲突做有界重试。
            Err(ts_rs::ExportError::Io(error))
                if matches!(error.raw_os_error(), Some(5 | 32 | 33 | 1224)) =>
            {
                let Some(delay) = delays.next() else {
                    panic!("export {label}: {error:?}");
                };
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            Err(error) => panic!("export {label}: {error:?}"),
        }
    }
}

pub fn normalize_generated_bindings(manifest_dir: &str) {
    let bindings_dir = Path::new(manifest_dir).join("../frontend/src/bindings");
    for entry in std::fs::read_dir(bindings_dir).expect("read generated bindings directory") {
        let path = entry.expect("read generated binding entry").path();
        if path.extension().is_none_or(|extension| extension != "ts") {
            continue;
        }
        let contents = std::fs::read_to_string(&path).expect("read generated binding");
        let mut normalized = contents
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        if contents.ends_with('\n') {
            normalized.push('\n');
        }
        if normalized != contents {
            // 文件监听器也可能映射已生成文件，规范化写入采用相同的有界重试。
            let mut delays = [10, 20, 40, 80, 160, 320, 640].into_iter();
            loop {
                match std::fs::write(&path, &normalized) {
                    Ok(()) => break,
                    Err(error) if matches!(error.raw_os_error(), Some(5 | 32 | 33 | 1224)) => {
                        let Some(delay) = delays.next() else {
                            panic!("normalize generated binding {}: {error}", path.display());
                        };
                        std::thread::sleep(std::time::Duration::from_millis(delay));
                    }
                    Err(error) => panic!("normalize generated binding {}: {error}", path.display()),
                }
            }
        }
    }
}
