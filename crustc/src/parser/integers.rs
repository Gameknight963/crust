use super::*;

pub(super) fn literal_type(
    value: u64,
    suffix: crate::lexer::IntegerSuffix,
    decimal: bool,
    span: Span,
) -> Result<IntType, Diagnostic> {
    use crate::lexer::IntegerSuffix;
    let long = IntType::native_long();
    let wide = IntType::native_long_long();
    let candidates: &[IntType] = match suffix {
        IntegerSuffix::Unsigned => &[IntType::U32, long.unsigned(), wide.unsigned()],
        IntegerSuffix::Long if decimal => &[long, wide],
        IntegerSuffix::Long => &[long, long.unsigned(), wide, wide.unsigned()],
        IntegerSuffix::UnsignedLong => &[long.unsigned(), wide.unsigned()],
        IntegerSuffix::LongLong if decimal => &[wide],
        IntegerSuffix::LongLong => &[wide, wide.unsigned()],
        IntegerSuffix::UnsignedLongLong => &[wide.unsigned()],
    };
    return candidates
        .iter()
        .copied()
        .find(|kind| kind.fits(i128::from(value)))
        .ok_or_else(|| {
            Diagnostic::new(
                span,
                "integer literal does not fit any C type allowed by its suffix and base",
            )
        });
}

impl Parser {
    pub(super) fn integer_type(
        &mut self,
        mut token: SpannedToken,
        qualifiers: &mut Qualifiers,
    ) -> Result<Type, Diagnostic> {
        let start = token.span;
        let mut signed = 0;
        let mut unsigned = 0;
        let mut short = 0;
        let mut long = 0;
        let mut int = 0;
        let mut character = 0;
        loop {
            match token.kind {
                Token::Int => int += 1,
                Token::Char => character += 1,
                Token::Signed => signed += 1,
                Token::Unsigned => unsigned += 1,
                Token::Short => short += 1,
                Token::Long => long += 1,
                _ => unreachable!(),
            }
            let next = self.qualifiers();
            qualifiers.readonly |= next.readonly;
            qualifiers.volatile |= next.volatile;
            if !matches!(
                self.current().kind,
                Token::Int
                    | Token::Char
                    | Token::Signed
                    | Token::Unsigned
                    | Token::Short
                    | Token::Long
            ) {
                break;
            }
            token = self.advance();
        }
        if signed > 1
            || unsigned > 1
            || short > 1
            || long > 2
            || int > 1
            || character > 1
            || (signed > 0 && unsigned > 0)
            || (short > 0 && long > 0)
            || (character > 0 && int + short + long > 0)
        {
            return Err(Diagnostic::new(
                start.through(token.span),
                "invalid C integer type specifiers",
            ));
        }
        let kind = if character > 0 {
            if signed + unsigned > 0 {
                IntType::I8
            } else {
                IntType::Char
            }
        } else if short > 0 {
            IntType::I16
        } else if long == 1 {
            IntType::native_long()
        } else if long == 2 {
            IntType::native_long_long()
        } else {
            IntType::I32
        };
        return Ok(Type::Integer(if unsigned > 0 {
            kind.unsigned()
        } else {
            kind
        }));
    }
}
