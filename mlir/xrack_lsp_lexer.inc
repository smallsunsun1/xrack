// Included in MLIRServer.cpp next to the pinned MLIR lexer.
namespace xrack::lsp {

static void readStringAttributes(mlir::Lexer &lexer, llvm::SMLoc start,
                                 llvm::StringMap<llvm::SMRange> &ranges) {
  using mlir::Token;
  lexer.resetPointer(start.getPointer());
  auto token = lexer.lexToken();
  if (!token.is(Token::string))
    return;
  if (!lexer.lexToken().is(Token::l_paren))
    return;
  do {
    token = lexer.lexToken();
  } while (token.isAny(Token::percent_identifier, Token::hash_identifier,
                       Token::comma));
  if (!token.is(Token::r_paren))
    return;

  token = lexer.lexToken();
  // Skip the optional successor list.
  if (token.is(Token::l_square)) {
    do {
      token = lexer.lexToken();
    } while (token.isAny(Token::caret_identifier, Token::comma));
    if (!token.is(Token::r_square))
      return;
    token = lexer.lexToken();
  }

  auto dictionary = [&] {
    llvm::SmallVector<Token::Kind, 4> ends{Token::r_brace};
    Token name = lexer.lexToken();
    for (token = name; !token.isAny(Token::eof, Token::error);
         token = lexer.lexToken()) {
      if (ends.size() == 1 && token.is(Token::equal) &&
          (name.isAny(Token::bare_identifier, Token::string) ||
           name.isKeyword())) {
        token = lexer.lexToken();
        if (token.is(Token::string)) {
          auto spelling = token.getSpelling().drop_front().drop_back();
          ranges.try_emplace(
              name.is(Token::string) ? name.getStringValue()
                                     : name.getSpelling(),
              llvm::SMRange(llvm::SMLoc::getFromPointer(spelling.begin()),
                            llvm::SMLoc::getFromPointer(spelling.end())));
        }
      }
      switch (token.getKind()) {
      case Token::l_brace:
        ends.push_back(Token::r_brace);
        break;
      case Token::l_paren:
        ends.push_back(Token::r_paren);
        break;
      case Token::l_square:
        ends.push_back(Token::r_square);
        break;
      case Token::less:
        ends.push_back(Token::greater);
        break;
      default:
        if (token.is(ends.back())) {
          ends.pop_back();
          if (ends.empty())
            return;
        }
        break;
      }
      name = token;
    }
  };

  // XR emits inherent attributes as operation properties, before regions.
  // Also support a generic attribute dictionary on operations without regions.
  if (token.is(Token::less)) {
    if (!lexer.lexToken().is(Token::l_brace))
      return;
    dictionary();
    if (!lexer.lexToken().is(Token::greater))
      return;
    token = lexer.lexToken();
  }
  if (token.is(Token::l_brace))
    dictionary();
}

void readStringAttributes(llvm::SourceMgr &source, mlir::Operation *op,
                          llvm::SMLoc start,
                          llvm::StringMap<llvm::SMRange> &ranges) {
  mlir::Lexer lexer(source, op->getContext(), nullptr);
  readStringAttributes(lexer, start, ranges);
}

} // namespace xrack::lsp
