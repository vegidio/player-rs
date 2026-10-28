//! A small reader for the relocatable pkg-config files shipped in the `binaries-mpv` archives.
//!
//! It resolves the link line of `mpv` the way `pkg-config --static --libs mpv` would, without needing a `pkg-config`
//! binary on the host (Windows usually has none). Shared by `build.rs` and by the crate's `tests/pkgconfig.rs`, which
//! runs it against the `.pc` files of every supported OS.
//!
//! Modules come from two places:
//! - the archive's `lib/pkgconfig/`: bundled libraries, read with `Libs.private` and `Requires.private` because they
//!   are linked statically;
//! - the host's pkg-config directories, for modules the archive only names (the Linux display and audio stacks): read
//!   with `Libs` and `Requires` only, because they are linked dynamically.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// How a library is linked, matching the kinds of `cargo:rustc-link-lib`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Lib {
    Static(String),
    Dylib(String),
    Framework(String),
}

/// The resolved link line: native search paths and libraries, dependents before their dependencies.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LinkPlan {
    pub search_paths: Vec<PathBuf>,
    pub libs: Vec<Lib>,
}

/// Where to look for modules and which libraries the archive provides statically.
pub struct Resolver {
    /// Rust `target_os` value (`macos`, `linux`, `windows`).
    pub target_os: String,
    /// The archive's `lib/` directory; `.pc` files are read from its `pkgconfig/` subdirectory.
    pub lib_dir: PathBuf,
    /// Host pkg-config directories searched for modules the archive does not ship.
    pub system_dirs: Vec<PathBuf>,
    /// Link names (`-l<name>`) of the static libraries present in `lib_dir`.
    pub static_libs: HashSet<String>,
}

/// One parsed `.pc` file.
struct Module {
    system: bool,
    libs: Vec<String>,
    requires: Vec<String>,
}

impl Resolver {
    /// Creates a resolver for `lib_dir`, listing the static libraries it contains.
    pub fn new(target_os: &str, msvc: bool, lib_dir: &Path, system_dirs: Vec<PathBuf>) -> Result<Self, String> {
        let entries = fs::read_dir(lib_dir).map_err(|e| format!("cannot read {}: {e}", lib_dir.display()))?;
        let static_libs = entries
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                static_link_name(&name, msvc)
            })
            .collect();

        Ok(Self { target_os: target_os.to_string(), lib_dir: lib_dir.to_path_buf(), system_dirs, static_libs })
    }

    /// Resolves the full link line of `root` (normally `mpv`).
    pub fn resolve(&self, root: &str) -> Result<LinkPlan, String> {
        let mut modules = HashMap::new();
        let mut order = Vec::new();
        self.visit(root, &mut modules, &mut HashSet::new(), &mut order)?;

        // `order` is a post-order (dependencies first); reversed, every module precedes the modules it requires.
        let mut plan = LinkPlan::default();
        let mut libs = Vec::new();
        for name in order.iter().rev() {
            let module: &Module = &modules[name];
            self.map_tokens(&module.libs, module.system, &mut plan.search_paths, &mut libs)?;
        }

        plan.libs = dedup_keep_last(libs);
        Ok(plan)
    }

    fn visit(
        &self,
        name: &str,
        modules: &mut HashMap<String, Module>,
        in_progress: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) -> Result<(), String> {
        if modules.contains_key(name) || !in_progress.insert(name.to_string()) {
            return Ok(());
        }

        let module = self.load(name)?;
        for dep in &module.requires {
            self.visit(dep, modules, in_progress, order)?;
        }

        in_progress.remove(name);
        modules.insert(name.to_string(), module);
        order.push(name.to_string());
        Ok(())
    }

    /// Loads `<name>.pc` from the archive, falling back to the host's pkg-config directories.
    fn load(&self, name: &str) -> Result<Module, String> {
        let file = format!("{name}.pc");
        let archive_pc = self.lib_dir.join("pkgconfig").join(&file);
        if archive_pc.is_file() {
            return parse_file(&archive_pc, false);
        }

        if let Some(path) = self.system_dirs.iter().map(|dir| dir.join(&file)).find(|p| p.is_file()) {
            return parse_file(&path, true);
        }

        let searched: Vec<String> = self.system_dirs.iter().map(|d| d.display().to_string()).collect();
        Err(format!(
            "pkg-config module `{name}` is required by mpv but is neither in the archive nor in the system pkg-config \
             directories [{}]; install the development package that provides `{file}`",
            searched.join(", ")
        ))
    }

    /// Maps the `Libs` tokens of one module to search paths and libraries.
    fn map_tokens(
        &self,
        tokens: &[String],
        system: bool,
        search_paths: &mut Vec<PathBuf>,
        libs: &mut Vec<Lib>,
    ) -> Result<(), String> {
        let mut iter = tokens.iter();
        while let Some(token) = iter.next() {
            if let Some(dir) = token.strip_prefix("-L") {
                // System directories are already on the linker's default path; adding them to rustc's search path
                // could let a system static library shadow the archive's.
                let dir = normalize(Path::new(dir));
                if !system && !search_paths.contains(&dir) {
                    search_paths.push(dir);
                }
            } else if let Some(name) = token.strip_prefix("-l") {
                libs.push(self.lib(name));
            } else if token == "-framework" {
                let framework = iter.next().ok_or("`-framework` without a framework name")?;
                libs.push(Lib::Framework(framework.clone()));
            } else if token == "-pthread" {
                // On macOS and Windows threads come with the C runtime.
                if self.target_os == "linux" {
                    libs.push(Lib::Dylib("pthread".to_string()));
                }
            } else if token == "-Wl,-E" {
                // `luajit.pc`: exports the executable's symbols for LuaJIT's FFI. mpv does not need it.
            } else if let Some(lib) = static_lib_from_path(token) {
                let path = Path::new(token);
                let dir = normalize(path.parent().unwrap_or(Path::new("")));
                if !search_paths.contains(&dir) {
                    search_paths.push(dir);
                }
                libs.push(Lib::Static(lib));
            } else {
                return Err(format!("unsupported pkg-config flag `{token}`"));
            }
        }
        Ok(())
    }

    fn lib(&self, name: &str) -> Lib {
        // Apple's SDK no longer ships libstdc++; its C++ runtime is libc++.
        let name = if self.target_os == "macos" && name == "stdc++" { "c++" } else { name };
        if self.static_libs.contains(name) {
            Lib::Static(name.to_string())
        } else {
            Lib::Dylib(name.to_string())
        }
    }
}

/// Returns the `-l` name of a static library file: `lib<name>.a` on GNU/macOS, `<name>.lib` on MSVC (the extension is
/// matched case-insensitively and the stem is kept as-is, so `libcrypto.lib` links as `libcrypto`).
pub fn static_link_name(file_name: &str, msvc: bool) -> Option<String> {
    let stem = if msvc {
        let (stem, ext) = file_name.rsplit_once('.')?;
        ext.eq_ignore_ascii_case("lib").then_some(stem)?
    } else {
        file_name.strip_prefix("lib")?.strip_suffix(".a")?
    };
    (!stem.is_empty()).then(|| stem.to_string())
}

/// Recognises a static library given by path (the MinGW-style entries in the Windows `mpv.pc`).
fn static_lib_from_path(token: &str) -> Option<String> {
    if !token.contains(['/', '\\']) {
        return None;
    }
    let file_name = Path::new(token).file_name()?.to_str()?;
    static_link_name(file_name, true).or_else(|| static_link_name(file_name, false))
}

/// Resolves `.` and `..` lexically, so `lib/pkgconfig/../../lib` and `lib` are recognised as the same search path.
/// Unlike `fs::canonicalize`, it does not produce `\\?\` paths on Windows, which some linkers reject.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Removes duplicate libraries, keeping the last occurrence so that dependencies stay after their dependents.
fn dedup_keep_last(libs: Vec<Lib>) -> Vec<Lib> {
    let mut seen = HashSet::new();
    let mut out: Vec<Lib> = libs.into_iter().rev().filter(|lib| seen.insert(lib.clone())).collect();
    out.reverse();
    out
}

fn parse_file(path: &Path, system: bool) -> Result<Module, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let pcfiledir = path.parent().unwrap_or(Path::new("."));
    parse(&text, pcfiledir, system).map_err(|e| format!("{}: {e}", path.display()))
}

/// Parses the text of a `.pc` file. `${pcfiledir}` expands to the directory holding it, with forward slashes so that a
/// Windows path survives the backslash escapes of `split_args`.
fn parse(text: &str, pcfiledir: &Path, system: bool) -> Result<Module, String> {
    let pcfiledir = pcfiledir.display().to_string().replace('\\', "/");
    // `pc_sysrootdir` is predefined by pkg-config (`/` without a sysroot); Ubuntu's `wayland-protocols.pc` uses it.
    let mut vars =
        HashMap::from([("pcfiledir".to_string(), pcfiledir), ("pc_sysrootdir".to_string(), "/".to_string())]);
    let mut fields = HashMap::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // A line is a field (`Key: value`) or a variable (`key=value`), whichever separator comes first.
        let colon = line.find(':');
        let equals = line.find('=');
        match (colon, equals) {
            (Some(c), e) if e.is_none_or(|e| c < e) => {
                let value = expand(line[c + 1..].trim(), &vars)?;
                fields.insert(line[..c].trim().to_string(), value);
            }
            (_, Some(e)) => {
                let value = expand(line[e + 1..].trim(), &vars)?;
                vars.insert(line[..e].trim().to_string(), value);
            }
            _ => return Err(format!("unparsable line `{line}`")),
        }
    }

    let field = |key: &str| fields.get(key).map(String::as_str).unwrap_or("");
    let mut libs = split_args(field("Libs"))?;
    let mut requires = parse_requires(field("Requires"));
    if !system {
        libs.extend(split_args(field("Libs.private"))?);
        requires.extend(parse_requires(field("Requires.private")));
    }

    Ok(Module { system, libs, requires })
}

/// Expands `${var}` references with the variables defined so far.
fn expand(value: &str, vars: &HashMap<String, String>) -> Result<String, String> {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let end = rest[start..].find('}').ok_or_else(|| format!("unterminated variable in `{value}`"))? + start;
        let name = &rest[start + 2..end];
        out.push_str(vars.get(name).ok_or_else(|| format!("undefined variable `{name}`"))?);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Splits a `Libs` value into arguments, honouring double/single quotes and backslash escapes.
fn split_args(value: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_arg = false;
    let mut quote = None;
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => {
                quote = Some(ch);
                in_arg = true;
            }
            (_, '\\') => {
                current.push(chars.next().ok_or_else(|| format!("trailing backslash in `{value}`"))?);
                in_arg = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            (_, c) => {
                current.push(c);
                in_arg = true;
            }
        }
    }

    if quote.is_some() {
        return Err(format!("unterminated quote in `{value}`"));
    }
    if in_arg {
        args.push(current);
    }
    Ok(args)
}

/// Parses a `Requires` value into module names, dropping version constraints (`libavcodec >= 60.31.102`).
fn parse_requires(value: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut tokens = value.split([',', ' ', '\t']).filter(|t| !t.is_empty());
    while let Some(token) = tokens.next() {
        if matches!(token, "=" | "!=" | "<" | ">" | "<=" | ">=") {
            tokens.next(); // the version
        } else {
            names.push(token.to_string());
        }
    }
    names
}
