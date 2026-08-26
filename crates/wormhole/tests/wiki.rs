//! The wiki this repository publishes. `docs/wiki.sh` turns the book's
//! markdown into flat GitHub wiki pages: the wiki has no directories, and a
//! link it cannot resolve is a link the wiki would serve broken, so the
//! script has to fail rather than publish one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

mod common;
use common::repo_root;

fn write(root: &Path, path: &str, body: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().expect("a page has a parent directory"))
        .expect("page directory");
    fs::write(&file, body).expect("page");
}

fn render(pages: &Path, summary: &Path, out: &Path) -> Output {
    Command::new("/bin/sh")
        .arg(repo_root().join("docs/wiki.sh"))
        .arg(pages)
        .arg(summary)
        .arg(out)
        .output()
        .expect("docs/wiki.sh runs")
}

/// A fixture book: pages, the summary the sidebar is built from, and the
/// wiki they render into, all under one temporary directory.
struct Book(TempDir);

impl Book {
    fn new() -> Self {
        let book = Book(tempfile::tempdir().expect("temporary directory"));
        write(
            &book.pages(),
            "SUMMARY.md",
            "# Summary\n\n[Home](index.md)\n",
        );
        book
    }

    fn pages(&self) -> PathBuf {
        self.0.path().join("pages")
    }

    fn summary(&self) -> PathBuf {
        self.pages().join("SUMMARY.md")
    }

    fn out(&self) -> PathBuf {
        self.0.path().join("wiki")
    }

    fn page(&self, path: &str, body: &str) -> &Self {
        write(&self.pages(), path, body);
        self
    }

    fn render(&self) -> Output {
        render(&self.pages(), &self.summary(), &self.out())
    }

    fn rendered(&self) -> &Self {
        let output = self.render();
        assert!(
            output.status.success(),
            "the wiki did not render: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        self
    }

    fn wiki_page(&self, name: &str) -> String {
        fs::read_to_string(self.out().join(name))
            .unwrap_or_else(|_| panic!("the wiki has no page {name}"))
    }
}

#[test]
fn every_page_becomes_one_flat_wiki_page() {
    let book = Book::new();
    book.page("index.md", "# Home\n")
        .page("guide/install.md", "# Install\n")
        .rendered();

    assert_eq!(book.wiki_page("Home.md"), "# Home\n");
    assert_eq!(book.wiki_page("guide-install.md"), "# Install\n");
}

#[test]
fn links_between_pages_point_at_wiki_page_names() {
    let book = Book::new();
    book.page("index.md", "[Install](guide/install.md)\n")
        .page(
            "guide/install.md",
            "[Boxes](boxes.md) [Home](../index.md) [Step](boxes.md#step-two)\n",
        )
        .page("guide/boxes.md", "# Boxes\n")
        .rendered();

    assert_eq!(book.wiki_page("Home.md"), "[Install](guide-install)\n");
    assert_eq!(
        book.wiki_page("guide-install.md"),
        "[Boxes](guide-boxes) [Home](Home) [Step](guide-boxes#step-two)\n"
    );
}

#[test]
fn links_that_do_not_name_a_page_are_left_alone() {
    let untouched = "[Repo](https://github.com/naborka/wormhole/blob/dev/PLAN.md)\n\
                     [Below](#the-manifest)\n\
                     [Manifest](wormhole.toml)\n";
    let book = Book::new();
    book.page("index.md", untouched).rendered();

    assert_eq!(book.wiki_page("Home.md"), untouched);
}

#[test]
fn links_inside_a_fenced_block_are_left_alone() {
    let book = Book::new();
    book.page(
        "index.md",
        "```\nsee [Install](guide/install.md)\n```\n[Install](guide/install.md)\n",
    )
    .page("guide/install.md", "# Install\n")
    .rendered();

    assert_eq!(
        book.wiki_page("Home.md"),
        "```\nsee [Install](guide/install.md)\n```\n[Install](guide-install)\n"
    );
}

#[test]
fn a_link_to_a_page_that_does_not_exist_fails_the_render() {
    let book = Book::new();
    let output = book.page("index.md", "[Gone](guide/gone.md)\n").render();

    assert!(!output.status.success(), "a broken link must not publish");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("guide/gone.md"),
        "the broken link has to be named"
    );
}

#[test]
fn pages_that_want_one_wiki_page_name_fail_the_render() {
    let book = Book::new();
    let output = book
        .page("index.md", "# Home\n")
        .page("guide-boxes.md", "# Boxes\n")
        .page("guide.boxes.md", "# Boxes again\n")
        .page("guide/boxes.md", "# Boxes once more\n")
        .render();

    assert!(!output.status.success(), "a collision must not publish");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("guide-boxes"),
        "the contested name has to be named"
    );
}

#[test]
fn a_page_that_wants_a_name_the_wiki_reserves_fails_the_render() {
    for reserved in ["_Sidebar", "_Footer"] {
        let book = Book::new();
        let output = book
            .page("index.md", "# Home\n")
            .page(&format!("{reserved}.md"), "# Mine now\n")
            .render();

        assert!(
            !output.status.success(),
            "a page must not overwrite the wiki's own {reserved}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reserved),
            "the contested name has to be named"
        );
    }
}

#[test]
fn the_summary_becomes_the_sidebar_and_is_not_a_page_of_its_own() {
    let book = Book::new();
    book.page("index.md", "# Home\n").rendered();

    assert_eq!(book.wiki_page("_Sidebar.md"), "# Summary\n\n[Home](Home)\n");
    assert!(
        !book.out().join("SUMMARY.md").exists(),
        "the summary is the sidebar, not a page"
    );
}

#[test]
fn every_page_says_the_wiki_is_generated() {
    let book = Book::new();
    book.page("index.md", "# Home\n").rendered();

    assert!(
        book.wiki_page("_Footer.md").contains("overwritten"),
        "someone editing the wiki has to be told their edit will not last"
    );
}

#[test]
fn the_docs_this_repository_ships_render_without_a_broken_link() {
    let wiki = tempfile::tempdir().expect("temporary directory");
    let docs = repo_root().join("docs/src");
    let output = render(&docs, &docs.join("SUMMARY.md"), &wiki.path().join("wiki"));

    assert!(
        output.status.success(),
        "the shipped docs do not render as a wiki: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        wiki.path().join("wiki/Home.md").is_file(),
        "the wiki needs a home page"
    );
}
