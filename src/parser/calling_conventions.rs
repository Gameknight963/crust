use super::*;

impl Parser {
    pub(super) fn calling_convention(&mut self) -> Result<Option<CallingConvention>, Diagnostic> {
        let convention = match self.current().kind {
            Token::Cdecl => CallingConvention::C,
            Token::Stdcall => CallingConvention::Stdcall,
            Token::Fastcall => CallingConvention::Fastcall,
            Token::Vectorcall => CallingConvention::Vectorcall,
            _ => return Ok(None),
        };
        let span = self.advance().span;
        if convention != CallingConvention::C && !cfg!(windows) {
            return Err(Diagnostic::new(
                span,
                "this calling convention requires a Windows target",
            ));
        }
        if convention == CallingConvention::Vectorcall
            && !cfg!(any(target_arch = "x86", target_arch = "x86_64"))
        {
            return Err(Diagnostic::new(
                span,
                "__vectorcall requires an x86 or x64 target",
            ));
        }
        return Ok(Some(convention));
    }

    pub(super) fn signature_convention(
        convention: CallingConvention,
        variadic: bool,
        span: Span,
    ) -> Result<CallingConvention, Diagnostic> {
        if variadic && convention == CallingConvention::Vectorcall {
            return Err(Diagnostic::new(span, "__vectorcall cannot be variadic"));
        }
        return Ok(convention.canonical(variadic));
    }
}
