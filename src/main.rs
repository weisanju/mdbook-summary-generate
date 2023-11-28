use clap::{Arg, ArgMatches, Command};
use mdbook::book::Book;
use mdbook::errors::Error;
use mdbook::preprocess::{CmdPreprocessor, Preprocessor, PreprocessorContext};
use semver::{Version, VersionReq};
use std::io;

use std::process;
use crate::nop_lib::{SummaryGenerate};

pub fn make_app() -> Command {
    Command::new("summary-generate-preprocessor")
        .about("A mdbook preprocessor which does precisely nothing")
        .subcommand(
            Command::new("supports")
                .arg(Arg::new("renderer").required(true))
                .about("Check whether a renderer is supported by this preprocessor"),
        )
}


fn handle_preprocessing(pre: &dyn Preprocessor) -> Result<(), Error> {
    let (ctx, book) = CmdPreprocessor::parse_input(io::stdin())?;
    let book_version = Version::parse(&ctx.mdbook_version)?;
    let version_req = VersionReq::parse(mdbook::MDBOOK_VERSION)?;
    if !version_req.matches(&book_version) {
        eprintln!(
            "Warning: The {} plugin was built against version {} of mdbook, \
             but we're being called from version {}",
            pre.name(),
            mdbook::MDBOOK_VERSION,
            ctx.mdbook_version
        );
    }

    let processed_book = pre.run(&ctx, book)?;
    serde_json::to_writer(io::stdout(), &processed_book)?;

    Ok(())
}

fn handle_supports(pre: &dyn Preprocessor, sub_args: &ArgMatches) -> ! {
    let renderer = sub_args
        .get_one::<String>("renderer")
        .expect("Required argument");

    let supported = pre.supports_renderer(renderer);
    // Signal whether the renderer is supported by exiting with 1 or 0.
    if supported {
        process::exit(0);
    } else {
        process::exit(1);
    }
}

fn main() {
    let matches = make_app().get_matches();
    let preprocessor = SummaryGenerate::new();
    if let Some(sub_args) = matches.subcommand_matches("supports") {
        handle_supports(&preprocessor, sub_args);
    } else if let Err(e) = handle_preprocessing(&preprocessor) {
        eprintln!("{}", e);
        process::exit(1);
    }
}

mod nop_lib {
    use std::cmp::Ordering;
    use std::cmp::Ordering::Equal;
    use std::fs;
    use std::path::{Path, PathBuf};
    use mdbook::book::{Chapter, SectionNumber};
    use mdbook::BookItem;
    use serde::{Serialize};
    use super::*;

    pub struct SummaryGenerate;

    impl SummaryGenerate {
        pub fn new() -> SummaryGenerate {
            SummaryGenerate {}
        }
    }

    #[derive(Serialize)]
    struct Item {
        name: String,
        /**
          分类
        **/
        type_name: String,

        children: Vec<Item>,

        path: PathBuf,
    }

    impl PartialOrd<Self> for Item {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    impl PartialEq<Self> for Item {
        fn eq(&self, other: &Self) -> bool {
            self.type_name == other.type_name && self.name == other.name
        }
    }

    impl Eq for Item {}

    impl Ord for Item {
        fn cmp(&self, other: &Self) -> Ordering {
            let ordering = self.type_name.cmp(&other.type_name);
            if ordering == Equal {
                self.name.cmp(&other.name)
            } else {
                ordering
            }
        }
    }

    impl Item {
        #[warn(dead_code)]
        fn recursive_handle(&self, level: usize, f: fn(&Item, usize)) {
            f(self, level);
            for x in &self.children {
                x.recursive_handle(level + 1, f)
            }
        }
    }

    fn visit_dirs_build(dir: &Path, _level: usize, parent: &mut Chapter, root: &PathBuf) -> io::Result<()> {
        if dir.ends_with("book") || dir.file_name().unwrap() == "images" {
            return Ok(());
        }

        if dir.is_dir() {
            let mut children: Vec<Chapter> = vec![];

            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = &entry.path();

                let name = path.file_name().map(|item| item.to_str()).unwrap().unwrap();

                let file_name = path.file_name().unwrap().to_str().unwrap();

                if path.is_dir() {
                    if !path.ends_with("book") || file_name != "images" {
                        let mut chapter = create_chapter(parent, path, file_name, root);
                        visit_dirs_build(path, _level + 1, &mut chapter, root)?;
                        children.push(chapter);
                    }
                } else if path.extension().unwrap_or_default() == "md"
                    && !name.eq_ignore_ascii_case("README.md")
                    && !name.eq_ignore_ascii_case("INDEX.md")
                    && !name.eq_ignore_ascii_case("SUMMARY.md")
                {
                    let chapter = create_chapter(parent, path, file_name, root);
                    children.push(chapter);
                }
            }
            children.sort_by(|a, b| {
                get_type_name_and_file_name(a.path.as_ref().unwrap())
                    .cmp(
                        &get_type_name_and_file_name(b.path.as_ref().unwrap())
                    )
            });

            let mut current_type = "".to_string();

            //handle index
            for (index, mut x) in children.into_iter().enumerate() {

                let mut section_number = parent.number.clone().unwrap_or_else(|| SectionNumber::from_iter(vec![]));
                section_number.push(index as u32);
                x.number = Some(section_number);

                handle_index(&mut x);

                let option = &x.path;
                let buf = option.as_ref().unwrap();
                let (type_name, _) = get_type_name_and_file_name(buf);
                let type_name = trim_number(type_name);
                if current_type != type_name {
                    current_type = type_name.to_string();
                    parent.sub_items.push(BookItem::Separator);
                    parent.sub_items.push(BookItem::PartTitle(type_name.to_string()));
                }
                parent.sub_items.push(BookItem::Chapter(x));
            }
        }
        Ok(())
    }


    fn handle_index(parent: &mut Chapter) {
        for (index, item) in parent.sub_items.iter_mut().enumerate() {
            if let BookItem::Chapter(data) = item {
                let mut section_number = parent.number.clone().unwrap_or_else(|| SectionNumber::from_iter(vec![]));
                section_number.push(index as u32);
                data.number = Some(section_number);
                handle_index(data);
            }
        }
    }

    /**
    去除前置数字
     **/
    fn trim_number(str: &str) -> &str {
        let mut index = 0;
        for x in str.chars() {
            if x.is_ascii_digit() || x == '.' {
                index += 1;
            } else {
                break;
            }
        }

        &str[index..]
    }

    fn get_type_name_and_file_name(name: &Path) -> (&str, &str) {
        let x = name.file_name().unwrap().to_str().unwrap();
        let index = x.find('_').unwrap_or(0);
        let type_name = if index > 0 {
            &x[0..index]
        } else {
            ""
        };
        (type_name, &x[index..])
    }


    fn create_chapter(parent: &Chapter, path: &Path, name: &str, root0: &PathBuf) -> Chapter {
        let mut relative = path.strip_prefix(root0).unwrap().to_path_buf();
        let content = if path.is_dir() {
            get_content_from_index(path, &mut relative)
        } else {
            fs::read_to_string(path).unwrap_or_default()
        };

        let name = trim_number(name);
        let mut vec = parent.parent_names.clone();
        vec.push(name.to_string());
        Chapter::new(name, content, relative, vec)
    }

    fn get_content_from_index(path: &Path, relative_name: &mut PathBuf) -> String {
        for x in ["INDEX.md", "README.md", "index.md", "readme.md"] {
            if let Some(data) = load_index_str(path, x) {
                relative_name.push(x);
                return data;
            }
        }
        "".to_string()
    }

    fn load_index_str(path: &Path, x: &str) -> Option<String> {
        let index = path.join(x);
        if index.exists() {
            Some(fs::read_to_string(index).unwrap_or_default())
        } else {
            None
        }
    }

    impl Preprocessor for SummaryGenerate {
        fn name(&self) -> &str {
            "summary-generate"
        }

        fn run(&self, ctx: &PreprocessorContext, mut book: Book) -> Result<Book, Error> {
            // if let Some(cfg) = ctx.config.get_preprocessor(self.name()) {}

            let root = &ctx.root.as_path().join("src");

            let mut root_chapter = Chapter::new("", "".to_string(), "", vec![]);

            root_chapter.number = Some(SectionNumber::from_iter(vec![]));

            visit_dirs_build(root, 0, &mut root_chapter, root)?;
            book.sections = root_chapter.sub_items;
            Ok(book)
        }
        fn supports_renderer(&self, renderer: &str) -> bool {
            renderer != "not-supported"
        }
    }


    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        fn summary_generate() {
            let input_json = r##"[
                {
                    "root": "/path/to/book",
                    "config": {
                        "book": {
                            "authors": ["AUTHOR"],
                            "language": "en",
                            "multilingual": false,
                            "src": "src",
                            "title": "TITLE"
                        },
                        "preprocessor": {
                            "summary-generate": {
                              "blow-up": true
                            }
                        }
                    },
                    "renderer": "html",
                    "mdbook_version": "0.4.35"
                },
                {
                    "sections": [
                        {
                            "Chapter": {
                                "name": "Chapter 1",
                                "content": "# Chapter 1\n",
                                "number": [1],
                                "sub_items": [],
                                "path": "chapter_1.md",
                                "source_path": "chapter_1.md",
                                "parent_names": []
                            }
                        }
                    ],
                    "__non_exhaustive": null
                }
            ]"##;
            let input_json = input_json.as_bytes();
            let (ctx, book) = CmdPreprocessor::parse_input(input_json).unwrap();
            let expected_book = book.clone();
            let result = SummaryGenerate::new().run(&ctx, book);
            assert!(result.is_ok());
            // The nop-preprocessor should not have made any changes to the book content.
            let actual_book = result.unwrap();
            assert_eq!(actual_book, expected_book);
        }
    }
}
