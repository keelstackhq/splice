//! `splice channel add|remove` (channels spec §3).

use std::collections::BTreeMap;

use semver::Version;

use crate::apply::{ApplyCtx, LockOut, apply, apply_files};
use crate::cli::{ChannelAddArgs, ChannelRemoveArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::filter::Filter;
use crate::fsio;
use crate::lock::{ChannelLock, ChannelMeta, Lock, valid_channel_name};
use crate::manifest::{PostCmd, Template, When};
use crate::output::{Action, Class, Report};
use crate::planner::{Disk, PlanInput, plan};
use crate::render::{RenderInput, RenderedSections, RenderedTree, render_sections, render_tree};
use crate::sections::{SectionInput, plan_sections};
use crate::source::TemplateSource;
use crate::sources::{Claim, SourceId, Sources, check_claims, claims_last};

type Result<T> = std::result::Result<T, SpliceError>;

fn usage<T>(msg: String) -> Result<T> {
    Err(SpliceError::Usage(msg))
}

/// `splice channel add`: like `update` with an empty base (channels spec §3). Order: dirty
/// check, resolve, render, filter, claim check across every source, plan files and sections,
/// refuse drift or broken markers (exit 1, nothing written), then files → sections → post →
/// the channel lock last. Shared-file conflicts get markers and the lock is written (G11).
pub fn add(g: &GlobalArgs, a: &ChannelAddArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    if !a.allow_dirty {
        super::update::require_clean(repo, "channel add")?;
    }
    let sources = Sources::load(repo)?;

    // Resolve: `--track` (branch head), `--ref`, or the latest tag (`--pre`).
    let src = TemplateSource::parse(&a.source)?;
    let fetcher = Fetcher::from_env();
    let resolved = match &a.track {
        Some(branch) => fetcher.branch_head(&src, branch)?,
        None => fetcher.resolve(&src, a.git_ref.as_deref(), a.pre)?,
    };
    let template = Template::load(&resolved.root)?;
    let running = Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version");
    template.check_min_splice(&running)?;
    let mut messages: Vec<String> = template.warnings.clone();

    // Name: `--as`, else the template's `name`.
    let name = a
        .as_name
        .clone()
        .unwrap_or_else(|| template.manifest.name.clone());
    if !valid_channel_name(&name) {
        return usage(format!(
            "invalid channel name `{name}`: expected `^[a-z][a-z0-9-]*$`; use --as NAME"
        ));
    }
    if sources.channels.iter().any(|c| c.meta.name == name) {
        return usage(format!(
            "channel `{name}` already exists; use splice update --channel {name}"
        ));
    }

    // Instances and vars (base + `--piece`), then render. A channel sees only its own vars.
    let data = super::new::parse_data(&a.data)?;
    let (vars, instances) =
        super::update::instances_for(&template, &template.manifest.base, &a.pieces, &data, g)?;
    let input = RenderInput {
        template_vars: vars.clone(),
        instances: instances.clone(),
        version: resolved.version.clone(),
    };
    let full = render_tree(&template, &input)?;
    let sections = render_sections(&template, &input)?;

    // Filter: dropped paths are neither written nor locked; a glob matching nothing is a note.
    let filter = Filter::new(&a.only, &a.exclude)?;
    let all_paths: Vec<&str> = full
        .keys()
        .chain(sections.keys())
        .map(String::as_str)
        .collect();
    messages.extend(filter.unused(&all_paths));
    let mut theirs = filter.apply_tree(full);
    let hosts: Vec<String> = sections
        .keys()
        .filter(|h| filter.keeps(h))
        .cloned()
        .collect();

    // One owner per path, across main, every channel and this one; nothing written on conflict.
    let me = SourceId::Channel(name.clone());
    let claims = claims_last(
        sources.claims(),
        Claim::from_plan(me.clone(), &theirs, hosts.iter().cloned()),
    );
    check_claims(&claims)?;
    // G10: a seed path another source locks is dropped from this plan.
    theirs.retain(|path, f| {
        f.class != Class::Seed
            || !claims
                .iter()
                .any(|c| c.source != me && c.files.contains(path))
    });

    // Plan files (base = empty, no locked files) and sections over one disk snapshot.
    let disk = Disk::snapshot(repo, theirs.keys().chain(hosts.iter()).map(String::as_str))?;
    let empty = RenderedTree::new();
    let none_locked = Lock::from_tree(&resolved, &vars, &instances, &empty);
    let fplan = plan(&PlanInput {
        base: &empty,
        theirs: &theirs,
        disk: &disk,
        lock: &none_locked,
        removes: Vec::new(),
        force: a.force,
    });
    let splan = plan_sections(&SectionInput {
        channel: &name,
        theirs: &sections,
        disk: &disk,
        locked: &BTreeMap::new(),
        filter: &filter,
        removing: false,
        force: a.force,
    });

    // Owned drift, section drift or broken markers: write nothing, exit 1.
    if !fplan.drift.is_empty() || !splan.drift.is_empty() || !splan.broken.is_empty() {
        let mut report = Report::new("channel add");
        report.ok = false;
        report.messages = messages;
        for path in fplan.drift.iter().chain(&splan.drift) {
            // Like `add`: a refused path is a `conflict`/`owned` change; sections are HOST#NAME.
            report.push(path, Action::Conflict, Class::Owned);
            report.msg(format!(
                "`{path}` differs from channel {name}'s version (edited locally)"
            ));
        }
        report.messages.extend(splan.broken.iter().cloned());
        report.msg(
            "nothing was written; revert or move the files above, pass --force to replace \
             them (broken markers are fixed by hand), or skip them with --exclude",
        );
        return Err(SpliceError::Drift(report));
    }

    let lock_new = ChannelLock {
        meta: ChannelMeta {
            name: name.clone(),
            track: a.track.clone(),
            only: a.only.clone(),
            exclude: a.exclude.clone(),
        },
        lock: Lock::from_tree(&resolved, &vars, &instances, &theirs),
        sections: splan.locked.clone(),
    };
    let lock_path = lock_new.path();
    let mut post: Vec<PostCmd> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for inst in &instances {
        if !seen.contains(&inst.id.as_str()) {
            seen.push(&inst.id);
            post.extend(template.pieces[&inst.id].post.iter().cloned());
        }
    }
    let lock_out = LockOut::Channel(lock_new);
    // No old lock exists: errors say none was written instead of "the lock still records".
    let still = lock_out.still_records(&resolved.version);
    let none_yet = format!("no {lock_path} was written");
    let result = apply(ApplyCtx {
        repo,
        plan: &fplan,
        steps: Vec::new(),
        from: resolved.version.clone(),
        to: resolved.version.clone(),
        lock: lock_out,
        post,
        when: When::Add,
        no_post: a.no_post,
        json: g.json,
        command: "channel add",
        sections: &splan.changes,
    });
    let fix = |mut r: Report| {
        for m in &mut r.messages {
            *m = m.replace(&still, &none_yet);
        }
        let mut all = messages.clone();
        all.append(&mut r.messages);
        r.messages = all;
        r
    };
    match result {
        Ok(r) => Ok(fix(r)),
        Err(SpliceError::Conflict(r)) => Err(SpliceError::Conflict(fix(r))),
        Err(SpliceError::Drift(r)) => Err(SpliceError::Drift(fix(r))),
        Err(SpliceError::Applied(r)) => Err(SpliceError::Applied(fix(r))),
        Err(SpliceError::Env(m)) => Err(SpliceError::Env(m.replace(&still, &none_yet))),
        Err(e) => Err(e),
    }
}

/// `splice channel remove`: every locked file and section is planned as "B only" (channels spec
/// §8). Unedited owned/shared files are deleted, edited ones are kept (owned drift refuses with
/// exit 1 unless `--force`; `--force` keeps them and says so, it never deletes them), seeds are
/// never touched, blocks are stripped (an edited block is kept the same way). The channel lock is
/// deleted last, so a failure leaves it and a re-run converges.
pub fn remove(g: &GlobalArgs, a: &ChannelRemoveArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    if !a.allow_dirty {
        super::update::require_clean(repo, "channel remove")?;
    }
    let sources = Sources::load(repo)?;
    let cl = sources.channel(&a.name)?;
    let name = a.name.as_str();
    let lock_path = cl.path();

    // A corrupt state where another source also tracks one of our paths: deleting would take a
    // file that source relies on. Sources::load does not check claims, so do it here.
    let mine = Claim::from_lock(SourceId::Channel(name.to_string()), &cl.lock, &cl.sections);
    for other in sources.claims().iter().filter(|c| c.source != mine.source) {
        let clash = |a: &std::collections::BTreeSet<String>,
                     b: &std::collections::BTreeSet<String>| {
            a.intersection(b).next().cloned()
        };
        let path = clash(&mine.files, &other.files)
            .or_else(|| clash(&mine.files, &other.hosts))
            .or_else(|| clash(&mine.hosts, &other.files));
        if let Some(path) = path {
            return Err(SpliceError::Env(format!(
                "path {path} is tracked by both {} and {}; nothing was removed; fix the lock by \
                 hand or re-add the channel",
                other.source, mine.source
            )));
        }
    }

    let sections_theirs = RenderedSections::new();
    let disk = Disk::snapshot(
        repo,
        cl.lock
            .files
            .keys()
            .chain(cl.sections.keys())
            .map(String::as_str),
    )?;

    // Files: an edited owned file is drift; an edited shared file is simply kept (shared files
    // are meant to be edited). Only the unedited ones go to the planner as "gone".
    let mut drift: Vec<String> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    let mut removes: Vec<String> = Vec::new();
    for (path, locked) in &cl.lock.files {
        let Some(bytes) = disk.files.get(path).and_then(|o| o.as_deref()) else {
            continue; // already gone
        };
        if fsio::sha256_hex(bytes) == locked.sha256 {
            removes.push(path.clone());
        } else if locked.class == Class::Owned && !a.force {
            drift.push(path.clone());
        } else {
            kept.push(format!("`{path}` was edited locally; kept"));
        }
    }
    let empty = RenderedTree::new();
    let fplan = plan(&PlanInput {
        base: &empty,
        theirs: &empty,
        disk: &disk,
        lock: &cl.lock,
        removes,
        force: a.force,
    });

    // Sections: an edited block is kept by the planner; without --force that is a refusal.
    let filter = Filter::new(&[], &[])?;
    let splan = plan_sections(&SectionInput {
        channel: name,
        theirs: &sections_theirs,
        disk: &disk,
        locked: &cl.sections,
        filter: &filter,
        removing: true,
        force: a.force,
    });
    let edited_blocks: Vec<String> = splan
        .changes
        .iter()
        .filter(|c| c.action == Action::Skip)
        .map(|c| c.report_path(name))
        .collect();
    if !a.force {
        drift.extend(edited_blocks);
    }

    if !drift.is_empty() || !splan.broken.is_empty() {
        let mut report = Report::new("channel remove");
        report.ok = false;
        for path in &drift {
            report.msg(format!(
                "`{path}` is owned by channel {name} but was edited locally"
            ));
        }
        report.messages.extend(splan.broken.iter().cloned());
        let mut hint = String::from("nothing was removed");
        if !drift.is_empty() {
            hint.push_str(
                "; revert the files above or pass --force to keep them and remove the rest",
            );
        }
        if !splan.broken.is_empty() {
            hint.push_str("; broken markers must be fixed by hand (--force does not help)");
        }
        report.msg(hint);
        return Err(SpliceError::Drift(report));
    }

    // Skipped (edited) blocks are reported as messages, not written.
    let sections: Vec<_> = splan
        .changes
        .iter()
        .filter(|c| c.action != Action::Skip)
        .cloned()
        .collect();
    let lock_out = LockOut::Channel(cl.clone());
    let version = lock_out.version().clone();
    let mut report = apply_files(&ApplyCtx {
        repo,
        plan: &fplan,
        steps: Vec::new(),
        from: version.clone(),
        to: version,
        lock: lock_out,
        post: Vec::new(),
        when: When::Update,
        no_post: true,
        json: g.json,
        command: "channel remove",
        sections: &sections,
    })?;
    report.messages.extend(kept);
    report
        .messages
        .extend(splan.changes.iter().filter_map(|c| c.note.clone()));

    // The lock goes last; the directory only when nothing else lives there.
    fsio::check_target(repo, &lock_path)?;
    fsio::remove_file(repo, &lock_path).map_err(|e| {
        SpliceError::Env(format!(
            "cannot delete {lock_path}: {e}; files and sections are already removed; re-run \
             `splice channel remove {name}` to finish, or `git checkout -- . && git clean -fd` \
             restores the previous state"
        ))
    })?;
    let dir = ".splice/channels";
    match fsio::check_target(repo, dir).and_then(|()| Ok(std::fs::remove_dir(repo.join(dir))?)) {
        Ok(()) => {}
        Err(SpliceError::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) => {}
        Err(e) => report.msg(format!("could not remove {dir}: {e}")),
    }
    Ok(report)
}
