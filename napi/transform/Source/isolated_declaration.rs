use std::path::Path;

use napi_derive::napi;

use oxc::{
    allocator::Allocator,
    codegen::{CodeGenerator, CommentOptions},
    isolated_declarations::IsolatedDeclarations,
    napi::{
        isolated_declarations::{IsolatedDeclarationsOptions, IsolatedDeclarationsResult},
        source_map::SourceMap,
    },
    parser::Parser,
    span::SourceType,
};

use crate::errors::wrap_diagnostics;

/// TypeScript Isolated Declarations for Standalone DTS Emit
#[allow(clippy::needless_pass_by_value)]
#[napi]
pub fn isolated_declaration(
    filename: String,
    source_text: String,
    options: Option<IsolatedDeclarationsOptions>,
) -> IsolatedDeclarationsResult {
    let source_path = Path::new(&filename);
    let source_type = SourceType::from_path(source_path).unwrap_or_default().with_typescript(true);
    let allocator = Allocator::default();
    let options = options.unwrap_or_default();

    let ret = Parser::new(&allocator, &source_text, source_type).parse();

    let transformed_ret = IsolatedDeclarations::new(
        &allocator,
        &source_text,
        &ret.trivias,
        oxc::isolated_declarations::IsolatedDeclarationsOptions {
            strip_internal: options.strip_internal.unwrap_or(false),
        },
    )
    .build(&ret.program);

    let mut codegen = CodeGenerator::new().enable_comment(
        &source_text,
        ret.trivias.clone(),
        CommentOptions { preserve_annotate_comments: false },
    );
    if options.sourcemap == Some(true) {
        codegen = codegen.enable_source_map(&filename, &source_text);
    }
    let codegen_ret = codegen.build(&transformed_ret.program);

    let errors = ret.errors.into_iter().chain(transformed_ret.errors).collect();
    let errors = wrap_diagnostics(source_path, source_type, &source_text, errors);

    IsolatedDeclarationsResult {
        code: codegen_ret.code,
        map: codegen_ret.map.map(SourceMap::from),
        errors,
    }
}