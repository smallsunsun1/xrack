// Generated from /Users/sunjiahe/coding/xrack/grammar/Xrack.g4 by ANTLR 4.13.1
import org.antlr.v4.runtime.tree.ParseTreeListener;

/**
 * This interface defines a complete listener for a parse tree produced by
 * {@link XrackParser}.
 */
public interface XrackListener extends ParseTreeListener {
	/**
	 * Enter a parse tree produced by {@link XrackParser#document}.
	 * @param ctx the parse tree
	 */
	void enterDocument(XrackParser.DocumentContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#document}.
	 * @param ctx the parse tree
	 */
	void exitDocument(XrackParser.DocumentContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#importDecl}.
	 * @param ctx the parse tree
	 */
	void enterImportDecl(XrackParser.ImportDeclContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#importDecl}.
	 * @param ctx the parse tree
	 */
	void exitImportDecl(XrackParser.ImportDeclContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#fontImport}.
	 * @param ctx the parse tree
	 */
	void enterFontImport(XrackParser.FontImportContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#fontImport}.
	 * @param ctx the parse tree
	 */
	void exitFontImport(XrackParser.FontImportContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#importName}.
	 * @param ctx the parse tree
	 */
	void enterImportName(XrackParser.ImportNameContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#importName}.
	 * @param ctx the parse tree
	 */
	void exitImportName(XrackParser.ImportNameContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#recordDecl}.
	 * @param ctx the parse tree
	 */
	void enterRecordDecl(XrackParser.RecordDeclContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#recordDecl}.
	 * @param ctx the parse tree
	 */
	void exitRecordDecl(XrackParser.RecordDeclContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#recordField}.
	 * @param ctx the parse tree
	 */
	void enterRecordField(XrackParser.RecordFieldContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#recordField}.
	 * @param ctx the parse tree
	 */
	void exitRecordField(XrackParser.RecordFieldContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#component}.
	 * @param ctx the parse tree
	 */
	void enterComponent(XrackParser.ComponentContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#component}.
	 * @param ctx the parse tree
	 */
	void exitComponent(XrackParser.ComponentContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#member}.
	 * @param ctx the parse tree
	 */
	void enterMember(XrackParser.MemberContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#member}.
	 * @param ctx the parse tree
	 */
	void exitMember(XrackParser.MemberContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#animation}.
	 * @param ctx the parse tree
	 */
	void enterAnimation(XrackParser.AnimationContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#animation}.
	 * @param ctx the parse tree
	 */
	void exitAnimation(XrackParser.AnimationContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#animationBody}.
	 * @param ctx the parse tree
	 */
	void enterAnimationBody(XrackParser.AnimationBodyContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#animationBody}.
	 * @param ctx the parse tree
	 */
	void exitAnimationBody(XrackParser.AnimationBodyContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#animationField}.
	 * @param ctx the parse tree
	 */
	void enterAnimationField(XrackParser.AnimationFieldContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#animationField}.
	 * @param ctx the parse tree
	 */
	void exitAnimationField(XrackParser.AnimationFieldContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#stateBlock}.
	 * @param ctx the parse tree
	 */
	void enterStateBlock(XrackParser.StateBlockContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#stateBlock}.
	 * @param ctx the parse tree
	 */
	void exitStateBlock(XrackParser.StateBlockContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#stateDecl}.
	 * @param ctx the parse tree
	 */
	void enterStateDecl(XrackParser.StateDeclContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#stateDecl}.
	 * @param ctx the parse tree
	 */
	void exitStateDecl(XrackParser.StateDeclContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#stateBinding}.
	 * @param ctx the parse tree
	 */
	void enterStateBinding(XrackParser.StateBindingContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#stateBinding}.
	 * @param ctx the parse tree
	 */
	void exitStateBinding(XrackParser.StateBindingContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#stateTransition}.
	 * @param ctx the parse tree
	 */
	void enterStateTransition(XrackParser.StateTransitionContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#stateTransition}.
	 * @param ctx the parse tree
	 */
	void exitStateTransition(XrackParser.StateTransitionContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#transitionAnimation}.
	 * @param ctx the parse tree
	 */
	void enterTransitionAnimation(XrackParser.TransitionAnimationContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#transitionAnimation}.
	 * @param ctx the parse tree
	 */
	void exitTransitionAnimation(XrackParser.TransitionAnimationContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#property}.
	 * @param ctx the parse tree
	 */
	void enterProperty(XrackParser.PropertyContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#property}.
	 * @param ctx the parse tree
	 */
	void exitProperty(XrackParser.PropertyContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#visibility}.
	 * @param ctx the parse tree
	 */
	void enterVisibility(XrackParser.VisibilityContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#visibility}.
	 * @param ctx the parse tree
	 */
	void exitVisibility(XrackParser.VisibilityContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#typeName}.
	 * @param ctx the parse tree
	 */
	void enterTypeName(XrackParser.TypeNameContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#typeName}.
	 * @param ctx the parse tree
	 */
	void exitTypeName(XrackParser.TypeNameContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#callback}.
	 * @param ctx the parse tree
	 */
	void enterCallback(XrackParser.CallbackContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#callback}.
	 * @param ctx the parse tree
	 */
	void exitCallback(XrackParser.CallbackContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#parameter}.
	 * @param ctx the parse tree
	 */
	void enterParameter(XrackParser.ParameterContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#parameter}.
	 * @param ctx the parse tree
	 */
	void exitParameter(XrackParser.ParameterContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#binding}.
	 * @param ctx the parse tree
	 */
	void enterBinding(XrackParser.BindingContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#binding}.
	 * @param ctx the parse tree
	 */
	void exitBinding(XrackParser.BindingContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#handler}.
	 * @param ctx the parse tree
	 */
	void enterHandler(XrackParser.HandlerContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#handler}.
	 * @param ctx the parse tree
	 */
	void exitHandler(XrackParser.HandlerContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#statement}.
	 * @param ctx the parse tree
	 */
	void enterStatement(XrackParser.StatementContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#statement}.
	 * @param ctx the parse tree
	 */
	void exitStatement(XrackParser.StatementContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#assignmentOp}.
	 * @param ctx the parse tree
	 */
	void enterAssignmentOp(XrackParser.AssignmentOpContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#assignmentOp}.
	 * @param ctx the parse tree
	 */
	void exitAssignmentOp(XrackParser.AssignmentOpContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#element}.
	 * @param ctx the parse tree
	 */
	void enterElement(XrackParser.ElementContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#element}.
	 * @param ctx the parse tree
	 */
	void exitElement(XrackParser.ElementContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#conditionClause}.
	 * @param ctx the parse tree
	 */
	void enterConditionClause(XrackParser.ConditionClauseContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#conditionClause}.
	 * @param ctx the parse tree
	 */
	void exitConditionClause(XrackParser.ConditionClauseContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#repeatClause}.
	 * @param ctx the parse tree
	 */
	void enterRepeatClause(XrackParser.RepeatClauseContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#repeatClause}.
	 * @param ctx the parse tree
	 */
	void exitRepeatClause(XrackParser.RepeatClauseContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#expression}.
	 * @param ctx the parse tree
	 */
	void enterExpression(XrackParser.ExpressionContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#expression}.
	 * @param ctx the parse tree
	 */
	void exitExpression(XrackParser.ExpressionContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#conditional}.
	 * @param ctx the parse tree
	 */
	void enterConditional(XrackParser.ConditionalContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#conditional}.
	 * @param ctx the parse tree
	 */
	void exitConditional(XrackParser.ConditionalContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#logicalOr}.
	 * @param ctx the parse tree
	 */
	void enterLogicalOr(XrackParser.LogicalOrContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#logicalOr}.
	 * @param ctx the parse tree
	 */
	void exitLogicalOr(XrackParser.LogicalOrContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#logicalAnd}.
	 * @param ctx the parse tree
	 */
	void enterLogicalAnd(XrackParser.LogicalAndContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#logicalAnd}.
	 * @param ctx the parse tree
	 */
	void exitLogicalAnd(XrackParser.LogicalAndContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#equality}.
	 * @param ctx the parse tree
	 */
	void enterEquality(XrackParser.EqualityContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#equality}.
	 * @param ctx the parse tree
	 */
	void exitEquality(XrackParser.EqualityContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#comparison}.
	 * @param ctx the parse tree
	 */
	void enterComparison(XrackParser.ComparisonContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#comparison}.
	 * @param ctx the parse tree
	 */
	void exitComparison(XrackParser.ComparisonContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#additive}.
	 * @param ctx the parse tree
	 */
	void enterAdditive(XrackParser.AdditiveContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#additive}.
	 * @param ctx the parse tree
	 */
	void exitAdditive(XrackParser.AdditiveContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#multiplicative}.
	 * @param ctx the parse tree
	 */
	void enterMultiplicative(XrackParser.MultiplicativeContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#multiplicative}.
	 * @param ctx the parse tree
	 */
	void exitMultiplicative(XrackParser.MultiplicativeContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#unary}.
	 * @param ctx the parse tree
	 */
	void enterUnary(XrackParser.UnaryContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#unary}.
	 * @param ctx the parse tree
	 */
	void exitUnary(XrackParser.UnaryContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#primary}.
	 * @param ctx the parse tree
	 */
	void enterPrimary(XrackParser.PrimaryContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#primary}.
	 * @param ctx the parse tree
	 */
	void exitPrimary(XrackParser.PrimaryContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#call}.
	 * @param ctx the parse tree
	 */
	void enterCall(XrackParser.CallContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#call}.
	 * @param ctx the parse tree
	 */
	void exitCall(XrackParser.CallContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#arrayLiteral}.
	 * @param ctx the parse tree
	 */
	void enterArrayLiteral(XrackParser.ArrayLiteralContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#arrayLiteral}.
	 * @param ctx the parse tree
	 */
	void exitArrayLiteral(XrackParser.ArrayLiteralContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#recordLiteral}.
	 * @param ctx the parse tree
	 */
	void enterRecordLiteral(XrackParser.RecordLiteralContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#recordLiteral}.
	 * @param ctx the parse tree
	 */
	void exitRecordLiteral(XrackParser.RecordLiteralContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#recordValue}.
	 * @param ctx the parse tree
	 */
	void enterRecordValue(XrackParser.RecordValueContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#recordValue}.
	 * @param ctx the parse tree
	 */
	void exitRecordValue(XrackParser.RecordValueContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#imageUrl}.
	 * @param ctx the parse tree
	 */
	void enterImageUrl(XrackParser.ImageUrlContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#imageUrl}.
	 * @param ctx the parse tree
	 */
	void exitImageUrl(XrackParser.ImageUrlContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#imageSlice}.
	 * @param ctx the parse tree
	 */
	void enterImageSlice(XrackParser.ImageSliceContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#imageSlice}.
	 * @param ctx the parse tree
	 */
	void exitImageSlice(XrackParser.ImageSliceContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#imageEdge}.
	 * @param ctx the parse tree
	 */
	void enterImageEdge(XrackParser.ImageEdgeContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#imageEdge}.
	 * @param ctx the parse tree
	 */
	void exitImageEdge(XrackParser.ImageEdgeContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#markdown}.
	 * @param ctx the parse tree
	 */
	void enterMarkdown(XrackParser.MarkdownContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#markdown}.
	 * @param ctx the parse tree
	 */
	void exitMarkdown(XrackParser.MarkdownContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#gradient}.
	 * @param ctx the parse tree
	 */
	void enterGradient(XrackParser.GradientContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#gradient}.
	 * @param ctx the parse tree
	 */
	void exitGradient(XrackParser.GradientContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#gradientAngle}.
	 * @param ctx the parse tree
	 */
	void enterGradientAngle(XrackParser.GradientAngleContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#gradientAngle}.
	 * @param ctx the parse tree
	 */
	void exitGradientAngle(XrackParser.GradientAngleContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#gradientStop}.
	 * @param ctx the parse tree
	 */
	void enterGradientStop(XrackParser.GradientStopContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#gradientStop}.
	 * @param ctx the parse tree
	 */
	void exitGradientStop(XrackParser.GradientStopContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#reference}.
	 * @param ctx the parse tree
	 */
	void enterReference(XrackParser.ReferenceContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#reference}.
	 * @param ctx the parse tree
	 */
	void exitReference(XrackParser.ReferenceContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#identifier}.
	 * @param ctx the parse tree
	 */
	void enterIdentifier(XrackParser.IdentifierContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#identifier}.
	 * @param ctx the parse tree
	 */
	void exitIdentifier(XrackParser.IdentifierContext ctx);
}