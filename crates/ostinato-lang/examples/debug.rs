fn main() {
    let chunks = [
        "#Retry: 1\n",
        "#Retry: { max: int | *2 }\n",
        "{ max: int | *2 }\n",
        "max: int | *2\n",
        "int | *2\n",
        "*2\n",
        "2\n",
        "#\n",
    ];
    for (i, chunk) in chunks.iter().enumerate() {
        eprintln!("chunk {i}: {:?}", chunk);
        match ostinato_lang::lexer::lex(chunk) {
            Ok(tokens) => eprintln!("  ok {} tokens", tokens.len()),
            Err(e) => eprintln!("  err {e}"),
        }
    }
}
