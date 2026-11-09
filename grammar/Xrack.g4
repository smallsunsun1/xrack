grammar Xrack;

document: (importDecl | fontImport)* (recordDecl | component)+ EOF;
importDecl: 'import' '{' importName (',' importName)* ','? '}' 'from' STRING ';';
fontImport: 'import' STRING ';';
importName: identifier ('as' identifier)?;
recordDecl: 'export'? 'struct' identifier '{' recordField (',' recordField)* ','? '}';
recordField: identifier ':' typeName;
component: 'export'? 'component' identifier 'inherits' identifier '{' member* '}';
member: property | callback | binding | handler | element | animation | stateBlock;
animation: 'animate' identifier (',' identifier)* animationBody;
animationBody: '{' animationField* '}';
animationField: ('duration' | identifier) ':' (NUMBER 'ms' | expression) ';';
stateBlock: 'states' '[' stateDecl* ']';
stateDecl: identifier ('when' expression)? ':' '{' (stateBinding | stateTransition)* '}';
stateBinding: reference ':' expression ';';
stateTransition: ('in' | 'out' | 'in-out') '{' transitionAnimation* '}';
transitionAnimation: 'animate' (reference (',' reference)* | '*') animationBody;
property: visibility? 'property' '<' typeName '>' identifier (':' expression)? ';';
visibility: 'in' | 'out' | 'in-out' | 'private';
typeName: identifier | '[' identifier ']';
callback: 'pure'? 'callback' identifier ('(' (parameter (',' parameter)* ','?)? ')' ('->' typeName)?)? ';';
parameter: (identifier ':')? typeName;
binding: identifier (':' expression | '<=>' reference) ';';
handler: identifier ('(' (identifier (',' identifier)*)? ')')? '=>' '{' statement* (expression ';'?)? '}';
statement: (reference assignmentOp expression | call) ';';
assignmentOp: '=' | '+=' | '-=';
element: (conditionClause | repeatClause)? (identifier ':=')? identifier '{' member* '}';
conditionClause: 'if' expression ':';
repeatClause: 'for' identifier ('[' identifier ']')? 'in' expression ':';

expression: conditional;
conditional: logicalOr ('?' expression ':' expression)?;
logicalOr: logicalAnd ('||' logicalAnd)*;
logicalAnd: equality ('&&' equality)*;
equality: comparison (('==' | '!=') comparison)*;
comparison: additive (('<' | '<=' | '>' | '>=') additive)*;
additive: multiplicative (('+' | '-') multiplicative)*;
multiplicative: unary (('*' | '/' | '%') unary)*;
unary: ('!' | '-' | '+') unary | primary;
primary: NUMBER (UNIT | '%')? | STRING | COLOR | 'true' | 'false' | gradient | imageUrl | markdown | arrayLiteral | recordLiteral | call | reference | '(' expression ')';
call: reference '(' (expression (',' expression)* ','?)? ')';
arrayLiteral: '[' (expression (',' expression)* ','?)? ']';
recordLiteral: identifier '{' (recordValue (',' recordValue)* ','?)? '}';
recordValue: identifier ':' expression;
imageUrl: '@image-url' '(' STRING ')';
markdown: '@markdown' '(' STRING ')';
gradient: ('@linear-gradient' '(' gradientAngle | '@radial-gradient' '(' 'circle')
          ',' gradientStop (',' gradientStop)* ')';
gradientAngle: ('+' | '-')? NUMBER 'deg';
gradientStop: expression (NUMBER '%')?;
reference: ('root' | 'self' | identifier) ('.' identifier)*;
// Type words are contextual: `color: ...` is an ordinary element binding.
identifier: IDENT | 'int' | 'float' | 'bool' | 'string' | 'length' | 'color' | 'brush' | 'image'
          | 'ImageFit' | 'LayoutAlignment' | 'CrossAxisAlignment' | 'Orientation' | 'DialInputMode' | 'ScrollBarPolicy' | 'TextWrap' | 'DialogRole' | 'DrawerEdge' | 'SelectionTarget' | 'circle' | 'deg' | 'duration' | 'ms' | 'as' | 'from' | 'pure' | 'struct' | 'states' | 'when';

UNIT: 'px';
COLOR: '#' [0-9a-fA-F]+;
NUMBER: [0-9]+ ('.' [0-9]+)? ([eE] [+-]? [0-9]+)?;
STRING: '"' (ESC | ~["\\\r\n])* '"';
fragment ESC: '\\' (["\\/bfnrt] | 'u' [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F]);
IDENT: [a-zA-Z_] [a-zA-Z_0-9]* ('-' [a-zA-Z_0-9]+)*;
LINE_COMMENT: '//' ~[\r\n]* -> skip;
BLOCK_COMMENT: '/*' .*? '*/' -> skip;
WS: [ \t\r\n\uFEFF]+ -> skip;
