//! Chargement d'un projet MCF (dossier de `.mcf`, `.uimcf`, `.cfg`) et ouverture d'une session.
//!
//! Les fichiers sont **non fiables** : le projet passe d'abord la vérification complète du
//! compilateur (`check_project`), et rien n'est exécuté s'il y a une seule erreur.

use std::path::{Path, PathBuf};

use mcf_diagnostics::SourceFile;
use mcf_runtime::{ViewSession, ui_stub_modules};
use mcf_semantic::{Source, check_project};

/// Nombre de fichiers au plus dans un projet d'interface (garde-fou contre un dossier énorme).
const MAX_FILES: usize = 2_000;
/// Taille maximale d'un fichier source.
const MAX_FILE_BYTES: u64 = 1_000_000;

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if !name.starts_with('.') && name != "target" {
                collect(&path, out)?;
            }
        } else if path
            .extension()
            .is_some_and(|e| e == "mcf" || e == "uimcf" || e == "cfg")
        {
            out.push(path);
        }
        if out.len() > MAX_FILES {
            return Err(std::io::Error::other("trop de fichiers dans le projet"));
        }
    }
    Ok(())
}

/// Lit et vérifie le projet du dossier `dir`, puis ouvre une session sur le composant
/// `component`. En cas d'erreur, rend le message complet (diagnostics du compilateur).
///
/// Les fichiers analysés sont **conservés pour toute la durée du programme** (`Box::leak`) : la
/// session les emprunte. Un rechargement à chaud en laissera donc quelques-uns en mémoire ; à
/// remplacer par une session qui possède ses données quand le rechargement sera écrit.
pub fn open(dir: &Path, component: &str) -> Result<ViewSession<'static>, String> {
    let mut paths = Vec::new();
    collect(dir, &mut paths).map_err(|e| format!("impossible de lire `{}` : {e}", dir.display()))?;
    paths.sort();
    let mut sources = Vec::new();
    for path in &paths {
        let meta = std::fs::metadata(path).map_err(|e| format!("{} : {e}", path.display()))?;
        if meta.len() > MAX_FILE_BYTES {
            return Err(format!("`{}` est trop gros", path.display()));
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{} : {e}", path.display()))?;
        sources.push(Source {
            name: path.display().to_string().replace('\\', "/"),
            text: text.strip_prefix('\u{feff}').unwrap_or(&text).to_string(),
        });
    }

    let files: Vec<SourceFile> = sources
        .iter()
        .map(|s| SourceFile::new(s.name.clone(), s.text.clone()))
        .collect();
    let mut report = String::new();
    let mut errors = 0usize;
    for (file, diags) in files.iter().zip(check_project(&sources)) {
        for diag in diags {
            if diag.is_hint() {
                continue;
            }
            errors += usize::from(diag.is_error());
            report.push_str(&file.render(&diag));
            report.push('\n');
        }
    }
    if errors > 0 {
        return Err(format!("{report}{errors} erreur(s) : le projet n'est pas chargé"));
    }

    let is_ui = |s: &Source| s.name.ends_with(".uimcf");
    let is_code = |s: &Source| s.name.ends_with(".mcf");
    let code: Vec<usize> = (0..sources.len()).filter(|&i| is_code(&sources[i])).collect();
    let uis: Vec<usize> = (0..sources.len()).filter(|&i| is_ui(&sources[i])).collect();
    // Code puis interfaces, comme l'attend le runtime.
    let ordered: Vec<SourceFile> = code
        .iter()
        .chain(&uis)
        .map(|&i| SourceFile::new(sources[i].name.clone(), sources[i].text.clone()))
        .collect();
    let dirs: Vec<PathBuf> = ordered
        .iter()
        .map(|f| {
            Path::new(&f.name)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default()
        })
        .collect();

    let code_parsed: Vec<_> = code
        .iter()
        .map(|&i| mcf_syntax::parse(&sources[i].text))
        .collect();
    let ui_parsed: Vec<_> = uis
        .iter()
        .map(|&i| mcf_syntax::parse_ui(&sources[i].text))
        .collect();
    let code_parsed: &'static Vec<_> = Box::leak(Box::new(code_parsed));
    let ui_parsed: &'static Vec<_> = Box::leak(Box::new(ui_parsed));
    let ui_modules: &'static Vec<_> =
        Box::leak(Box::new(ui_parsed.iter().map(|p| &p.module).collect::<Vec<_>>()));
    let stubs: &'static Vec<_> = Box::leak(Box::new(ui_stub_modules(ui_modules)));
    let modules: &'static Vec<_> = Box::leak(Box::new(
        code_parsed
            .iter()
            .map(|p| &p.module)
            .chain(stubs.iter())
            .collect::<Vec<_>>(),
    ));
    let ordered: &'static Vec<SourceFile> = Box::leak(Box::new(ordered));
    let dirs: &'static Vec<PathBuf> = Box::leak(Box::new(dirs));
    Ok(ViewSession::new(
        modules, ui_modules, ordered, dirs, dir, component,
    ))
}
