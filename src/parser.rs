use syn::{
    parenthesized, parse::Parse, parse::ParseStream, token::Paren, token, Block, Ident, Type,
    LitStr, Token,
};


#[derive(Debug)]
pub enum ParseTree {
    // list of all the non-terminals in this grammar
    DefinitionList(Vec<ParseTree>),
    // definition of a new non-terminal
    ParserDefinition(Ident, Option<Type>, Box<ParseTree>),

    // the sub-parser is wrapped in `<...>` or `<ident: ...>`
    Capture(Box<ParseTree>, Option<Ident>),

    // identifier, must be a non-terminal in this grammar
    NonTerminal(Ident),
    // call to external parser
    Call(Ident),
    // sequence of sub-parsers, with optional action code at the end
    Sequence(Vec<ParseTree>, Option<Block>),
    // terminal that consumes no input, equivalend to `""`
    Empty,
    // string literal
    Terminal(String),
    // ordered list of alternative sub-parsers
    Choice(Vec<ParseTree>),
    // Repetition: 0 or more times
    Many0(Box<ParseTree>),
    // Repetition: 1 or more times
    Many1(Box<ParseTree>),
    // makes the sub-parser optional
    Optional(Box<ParseTree>),
    // evaluates the sub-parser without consuming input
    Peek(Box<ParseTree>),
    // negates the result of the sub-parser
    Not(Box<ParseTree>),
}


enum Prefix {
    Peek,
    Not,
}

enum Postfix {
    Optional,
    Many0,
    Many1,
}

fn parse_prefix(input: ParseStream) -> Option<Prefix> {
    let lookahead = input.lookahead1();
    if lookahead.peek(Token![&]) {
        // Peek
        input.parse::<Token![&]>().unwrap(); // just skip past this
        Some(Prefix::Peek)
    } else if lookahead.peek(Token![!]) {
        // Not
        input.parse::<Token![!]>().unwrap(); // just skip past this
        Some(Prefix::Not)
    } else {
        // No prefix found
        None
    }
}

fn parse_postfix(input: ParseStream) -> Option<Postfix> {
    let lookahead = input.lookahead1();
    if lookahead.peek(Token![?]) {
        // Optional
        input.parse::<Token![?]>().unwrap(); // just skip past this
        Some(Postfix::Optional)
    } else if lookahead.peek(Token![*]) {
        // Many0
        input.parse::<Token![*]>().unwrap(); // just skip past this
        Some(Postfix::Many0)
    } else if lookahead.peek(Token![+]) {
        // Many1
        input.parse::<Token![+]>().unwrap(); // just skip past this
        Some(Postfix::Many1)
    } else {
        // No postfix found
        None
    }
}

fn parse_element(input: ParseStream) -> syn::Result<ParseTree> {
    let prefix = parse_prefix(input);

    let lookahead = input.lookahead1();

    let mut parsed = if lookahead.peek(Ident) {
        // if there's an '=' sign following it's the start of a new definition
        if parse_definition(&input.fork()).is_ok() {
        // if (input.peek2(Token![=]) && !input.peek2(Token![=>])) || input.peek2(Token![:]) {
            Err(input.error("Reached start of new definition."))
        } else {
            // Non-Terminal / Indentifier
            Ok(ParseTree::NonTerminal(input.parse::<Ident>()?))
        }
    } else if lookahead.peek(Token![::]) {
        // external function call
        input.parse::<Token![::]>()?;
        Ok(ParseTree::Call(input.parse::<Ident>()?))
    } else if lookahead.peek(LitStr) {
        // Terminal
        Ok(ParseTree::Terminal(input.parse::<LitStr>()?.value()))
    } else if lookahead.peek(Paren) {
        // Grouping
        // Get content of parens
        let content;
        parenthesized!(content in input);
        // and parse the content
        // Ok(ParseTree::Grouping(Box::new(content.parse::<ParseTree>()?)))
        Ok(parse_expression(&content)?)
    } else if lookahead.peek(Token![<]) {
        // Capture
        // token::Lt is Token![<], but that messed up my syntax highlighting
        input.parse::<token::Lt>()?; // just skip past this

        let ident = if input.peek(Ident) && input.peek2(Token![:]) {
            // identifier
            let i = Some(input.parse::<Ident>()?);
            // and then a colon
            input.parse::<Token![:]>()?; // just skip past this
            i
        } else {
            None
        };

        let term = parse_element(&input)?;
        input.parse::<token::Gt>()?; // just skip past this

        Ok(ParseTree::Capture(Box::new(term), ident))
    } else {
        Err(lookahead.error())
    };

    let postfix = parse_postfix(input);

    // process postfix
    parsed = parsed.and_then(|p| {
        Ok(match postfix {
            Some(Postfix::Optional) => ParseTree::Optional(Box::new(p)),
            Some(Postfix::Many0) => ParseTree::Many0(Box::new(p)),
            Some(Postfix::Many1) => ParseTree::Many1(Box::new(p)),
            None => p,
        })
    });

    // process prefix
    parsed.and_then(|p| {
        Ok(match prefix {
            Some(Prefix::Peek) => ParseTree::Peek(Box::new(p)),
            Some(Prefix::Not) => ParseTree::Not(Box::new(p)),
            None => p,
        })
    })
}

fn parse_sequence(input: ParseStream) -> syn::Result<ParseTree> {
    let mut expressions: Vec<ParseTree> = Vec::with_capacity(4);

    while !input.is_empty() {
        match parse_element(input) {
            Ok(e) => expressions.push(e),
            Err(_) => break,
        }
    }

    if expressions.len() == 0 {
        return Err(input.error("Need at least one element in a sequence"))
    }

    // let seq = match expressions.len() {
    //     0 => Ok(ParseTree::Empty),
    //     1 => Ok(expressions.remove(0)),
    //     _ => Ok(ParseTree::Sequence(expressions)),
    // };

    // Parse action code
    let block = if input.peek(Token![=>]) {
        // need a '=>', otherwise we don't have any action code
        input.parse::<Token![=>]>()?; // just skip past this
        Some(input.parse::<Block>()?)
    } else {
        None
    };

    Ok(ParseTree::Sequence(expressions, block))
}

fn parse_expression(input: ParseStream) -> syn::Result<ParseTree> {
    let mut expressions: Vec<ParseTree> = Vec::with_capacity(4);

    expressions.push(parse_sequence(input)?);
    while !input.is_empty() && input.peek(Token![|]) {
        input.parse::<Token![|]>()?; // just skip past this
        expressions.push(parse_sequence(input)?);
    }

    match expressions.len() {
        0 => Ok(ParseTree::Empty),
        1 => Ok(expressions.remove(0)),
        _ => Ok(ParseTree::Choice(expressions)),
    }
}

fn parse_definition(input: ParseStream) -> syn::Result<ParseTree> {
    // parse name
    let name = input.parse::<Ident>()?;

    // optionally parse a type
    let return_type = if input.peek(Token![:]) {
        input.parse::<Token![:]>()?; // just skip past this
        Some(input.parse::<Type>()?)
    } else {
        None
    };

    // then an equals sign
    input.parse::<Token![=]>()?; // just skip past this

    // parse expression
    let expression = parse_expression(input)?;

    // Final ast node
    Ok(ParseTree::ParserDefinition(name, return_type, Box::new(expression)))
}

impl Parse for ParseTree {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut definitions: Vec<ParseTree> = Vec::with_capacity(4);

        definitions.push(parse_definition(input)?);
        while !input.is_empty() {
            definitions.push(parse_definition(input)?);
        }
        Ok(ParseTree::DefinitionList(definitions))
    }
}
