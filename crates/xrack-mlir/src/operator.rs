use crate::OperationRef;

/// Operator on a verified `xr.unary` expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOperator {
    Positive,
    Negate,
    Not,
}

impl UnaryOperator {
    pub fn from_operation(op: OperationRef<'_>) -> Result<Self, String> {
        Ok(match op.attr("value")? {
            "+" => Self::Positive,
            "-" => Self::Negate,
            "!" => Self::Not,
            value => return Err(format!("unknown unary operator `{value}`")),
        })
    }
}

/// Operator on a verified `xr.binary` expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

impl BinaryOperator {
    pub fn from_operation(op: OperationRef<'_>) -> Result<Self, String> {
        Ok(match op.attr("value")? {
            "+" => Self::Add,
            "-" => Self::Subtract,
            "*" => Self::Multiply,
            "/" => Self::Divide,
            "%" => Self::Remainder,
            "==" => Self::Equal,
            "!=" => Self::NotEqual,
            "<" => Self::Less,
            "<=" => Self::LessEqual,
            ">" => Self::Greater,
            ">=" => Self::GreaterEqual,
            "&&" => Self::And,
            "||" => Self::Or,
            value => return Err(format!("unknown binary operator `{value}`")),
        })
    }
}
