//! 隐藏调试参数解析（`--dump` / `--check` / `--set-pin` / `--new-note`）。
//!
//! 这些参数**不是**面向用户的入口（UI 里没有对应按钮），只服务于
//! `scripts/check-zorder.ps1` 与手工排查。命令行会被单实例插件转发到已运行的实例里
//! 执行（见 `lib.rs` 的 `run()`），所以"另开一个进程"这件事对用户是透明的：
//! 效果等同于"在正在跑的这个应用里执行一条命令"。
//!
//! 识别的参数：
//!
//! ```text
//! --dump                      把窗口 + 数据状态写成 JSON（默认 %TEMP%\sticky-dump.json）
//! --check                     采集同一份数据并断言 6 条层级不变量
//! --out <路径>                 --dump / --check 的输出路径（相对路径按调用者的 cwd 解析）
//! --set-pin <note_id|auto> <true|false>
//!                             走和 UI 完全相同的后端路径切换置顶
//! --new-note                  按 UI 相同路径新建一张便签（脚本准备"≥2 张"用）
//! --delete-note <note_id>     删除指定便签（脚本清理自己建的临时便签用）
//! --exit-after-dump           dump / check 完成后退出进程，退出码 = 断言结果
//! ```

/// 解析后的诊断命令
#[derive(Debug, Clone, Default)]
pub struct Cli {
    /// `--dump`：导出窗口与数据状态
    pub dump: bool,
    /// `--check`：执行不变量断言（内部会顺带写一份 dump）
    pub check: bool,
    /// `--out <路径>`：dump 输出路径
    pub out: Option<String>,
    /// `--set-pin <note_id|auto> <true|false>`
    pub set_pin: Option<(String, bool)>,
    /// `--new-note`：新建一张便签
    pub new_note: bool,
    /// `--delete-note <note_id>`：删除指定便签（清理脚本自己建的临时便签）
    pub delete_note: Option<String>,
    /// `--exit-after-dump`：执行完就退出，退出码表示断言结果
    pub exit_after_dump: bool,
}

impl Cli {
    /// 是否真的带了要执行的命令（只有 `--out` 不算）
    pub fn has_command(&self) -> bool {
        self.dump
            || self.check
            || self.set_pin.is_some()
            || self.new_note
            || self.delete_note.is_some()
    }
}

/// 从完整 argv（第 0 项是 exe 自己）里挑出诊断参数。
///
/// 一个诊断参数都没有时返回 `None`，调用方应回落到原有的"唤回便签"行为。
pub fn parse(argv: &[String]) -> Option<Cli> {
    let mut cli = Cli::default();
    let mut args = argv.iter().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dump" => cli.dump = true,
            "--check" => cli.check = true,
            "--new-note" => cli.new_note = true,
            "--exit-after-dump" => cli.exit_after_dump = true,
            "--out" => {
                if let Some(value) = args.next() {
                    cli.out = Some(value.clone());
                }
            }
            "--set-pin" => {
                let target = args.next().cloned();
                let pinned = args.next().cloned().as_deref().and_then(parse_bool);
                match (target, pinned) {
                    (Some(target), Some(pinned)) => cli.set_pin = Some((target, pinned)),
                    // 参数不完整不要静默吞掉：脚本拼错了要能一眼看到
                    _ => eprintln!("[diag] --set-pin 用法：--set-pin <note_id|auto> <true|false>"),
                }
            }
            "--delete-note" => match args.next() {
                Some(id) => cli.delete_note = Some(id.clone()),
                None => eprintln!("[diag] --delete-note 用法：--delete-note <note_id>"),
            },
            other => {
                if let Some(value) = other.strip_prefix("--out=") {
                    cli.out = Some(value.to_string());
                }
            }
        }
    }

    if cli.has_command() {
        Some(cli)
    } else {
        None
    }
}

/// `true/false` 也接受 1/0、on/off、yes/no（脚本里两种写法都常见）
fn parse_bool(raw: &str) -> Option<bool> {
    match raw.to_ascii_lowercase().as_str() {
        "true" | "1" | "on" | "yes" => Some(true),
        "false" | "0" | "off" | "no" => Some(false),
        _ => None,
    }
}
