use clap::{Parser, ValueEnum};
use proc_macro2::{Group, Ident, Spacing, TokenStream, TokenTree};
use std::{fs, io, path::PathBuf, process::ExitCode};
use xrack_compiler::{
    CompilerError, check_resources, compile_with_inputs, generate_rust, lower, mlir, parse,
    parse_document, semantic,
};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum Emit {
    #[default]
    Rust,
    Mlir,
    /// Print the lowered MLIR before native verification and optimization.
    MlirRaw,
    /// Print the syntax tree for this file.
    Ast,
    /// Print the resolved and typed frontend representation.
    Hir,
}

#[derive(Debug, Parser)]
#[command(
    name = "xrackc",
    version,
    about = "Compile XRack UI files with ANTLR4 and MLIR"
)]
struct Args {
    /// Source .xr file.
    #[arg(value_name = "INPUT.xr")]
    input: String,

    /// Output format.
    #[arg(long, value_enum, default_value_t)]
    emit: Emit,

    /// Verify the input and its resources.
    #[arg(long)]
    check: bool,

    /// Write to a file instead of stdout.
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Generate Rust and print Cargo rebuild directives for sources and assets.
    #[arg(long, requires = "output", conflicts_with_all = ["emit", "check"])]
    cargo: bool,

    /// Resolve runtime symbols through the alias supplied by xrack::include_modules!().
    #[arg(long, requires = "cargo")]
    cargo_facade: bool,

    /// Resolve images and fonts only from the declared assets.
    #[arg(long)]
    restrict_assets: bool,

    /// Declare an image or font asset. May be supplied multiple times.
    #[arg(long = "asset", value_name = "FILE")]
    assets: Vec<PathBuf>,

    /// Resolve imports only from the declared sources.
    #[arg(long)]
    restrict_sources: bool,

    /// Declare an imported .xr file. May be supplied multiple times.
    #[arg(long = "source", value_name = "FILE")]
    sources: Vec<PathBuf>,
}

fn run(args: Args) -> Result<(), CompilerError> {
    let source = fs::read_to_string(&args.input).map_err(|source| CompilerError::Io {
        path: (&args.input).into(),
        source,
    })?;
    let diagnostic = |diagnostics| CompilerError::Diagnostics {
        filename: args.input.clone(),
        diagnostics,
    };
    let assets =
        (args.restrict_assets || !args.assets.is_empty()).then_some(args.assets.as_slice());
    let sources =
        (args.restrict_sources || !args.sources.is_empty()).then_some(args.sources.as_slice());
    if args.cargo {
        let (rust, inputs) = compile_with_inputs(&source, &args.input, sources, assets)?;
        let directives = inputs
            .iter()
            .map(|path| {
                path.to_str()
                    .filter(|path| !path.contains(['\r', '\n']))
                    .ok_or_else(|| CompilerError::Io {
                        path: path.clone(),
                        source: io::Error::new(io::ErrorKind::InvalidInput, "invalid Cargo path"),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rust = if args.cargo_facade {
            use_facade_runtime(rust)
        } else {
            rust
        };
        write_output(args.output, rust_source(rust))?;
        for path in directives {
            println!("cargo::rerun-if-changed={path}");
        }
        return Ok(());
    }
    let doc = if matches!(args.emit, Emit::Ast) && !args.check {
        parse(&source).map_err(diagnostic)?
    } else {
        parse_document(&source, &args.input, sources)?
    };
    let text = match args.emit {
        _ if args.check => return check_resources(&lower(&doc, &args.input)?, &args.input, assets),
        Emit::Ast => serde_json::to_string_pretty(&doc)?,
        Emit::Hir => serde_json::to_string_pretty(&semantic::analyze(&doc).map_err(diagnostic)?)?,
        Emit::MlirRaw => mlir::lower(&doc, &args.input).map_err(diagnostic)?,
        Emit::Mlir => lower(&doc, &args.input)?.to_mlir(),
        Emit::Rust => {
            let tokens = generate_rust(&lower(&doc, &args.input)?, &args.input, assets)?;
            rust_source(tokens)
        }
    };
    write_output(args.output, text)
}

fn use_facade_runtime(tokens: TokenStream) -> TokenStream {
    let mut tokens = tokens.into_iter().peekable();
    let mut result = TokenStream::new();
    while let Some(token) = tokens.next() {
        match token {
            TokenTree::Group(group) => {
                let mut rewritten =
                    Group::new(group.delimiter(), use_facade_runtime(group.stream()));
                rewritten.set_span(group.span());
                result.extend([TokenTree::Group(rewritten)]);
            }
            TokenTree::Punct(first)
                if first.as_char() == ':' && first.spacing() == Spacing::Joint =>
            {
                let second = tokens.next_if(|next| {
                    matches!(next, TokenTree::Punct(punctuation) if punctuation.as_char() == ':')
                });
                if let Some(second) = second {
                    if let Some(runtime) = tokens.next_if(
                        |next| matches!(next, TokenTree::Ident(name) if name == "xrack_runtime"),
                    ) {
                        result.extend([TokenTree::Ident(Ident::new(
                            "__xrack_runtime",
                            runtime.span(),
                        ))]);
                    } else {
                        result.extend([TokenTree::Punct(first), second]);
                    }
                } else {
                    result.extend([TokenTree::Punct(first)]);
                }
            }
            other => result.extend([other]),
        }
    }
    result
}

fn rust_source(tokens: impl std::fmt::Display) -> String {
    format!("// Generated by xrackc. Edit the .xr source file.\n{tokens}\n")
}

fn write_output(output: Option<PathBuf>, text: String) -> Result<(), CompilerError> {
    if let Some(path) = output {
        fs::write(&path, text).map_err(|source| CompilerError::Io { path, source })?;
    } else {
        println!("{text}");
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
