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
	 * Enter a parse tree produced by {@link XrackParser#translationsDecl}.
	 * @param ctx the parse tree
	 */
	void enterTranslationsDecl(XrackParser.TranslationsDeclContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#translationsDecl}.
	 * @param ctx the parse tree
	 */
	void exitTranslationsDecl(XrackParser.TranslationsDeclContext ctx);
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
	 * Enter a parse tree produced by {@link XrackParser#enumDecl}.
	 * @param ctx the parse tree
	 */
	void enterEnumDecl(XrackParser.EnumDeclContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#enumDecl}.
	 * @param ctx the parse tree
	 */
	void exitEnumDecl(XrackParser.EnumDeclContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#enumVariant}.
	 * @param ctx the parse tree
	 */
	void enterEnumVariant(XrackParser.EnumVariantContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#enumVariant}.
	 * @param ctx the parse tree
	 */
	void exitEnumVariant(XrackParser.EnumVariantContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#enumFieldTypes}.
	 * @param ctx the parse tree
	 */
	void enterEnumFieldTypes(XrackParser.EnumFieldTypesContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#enumFieldTypes}.
	 * @param ctx the parse tree
	 */
	void exitEnumFieldTypes(XrackParser.EnumFieldTypesContext ctx);
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
	 * Enter a parse tree produced by {@link XrackParser#viewMatch}.
	 * @param ctx the parse tree
	 */
	void enterViewMatch(XrackParser.ViewMatchContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#viewMatch}.
	 * @param ctx the parse tree
	 */
	void exitViewMatch(XrackParser.ViewMatchContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#viewArm}.
	 * @param ctx the parse tree
	 */
	void enterViewArm(XrackParser.ViewArmContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#viewArm}.
	 * @param ctx the parse tree
	 */
	void exitViewArm(XrackParser.ViewArmContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#functionDecl}.
	 * @param ctx the parse tree
	 */
	void enterFunctionDecl(XrackParser.FunctionDeclContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#functionDecl}.
	 * @param ctx the parse tree
	 */
	void exitFunctionDecl(XrackParser.FunctionDeclContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#namedParameter}.
	 * @param ctx the parse tree
	 */
	void enterNamedParameter(XrackParser.NamedParameterContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#namedParameter}.
	 * @param ctx the parse tree
	 */
	void exitNamedParameter(XrackParser.NamedParameterContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#slotMarker}.
	 * @param ctx the parse tree
	 */
	void enterSlotMarker(XrackParser.SlotMarkerContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#slotMarker}.
	 * @param ctx the parse tree
	 */
	void exitSlotMarker(XrackParser.SlotMarkerContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#slotFill}.
	 * @param ctx the parse tree
	 */
	void enterSlotFill(XrackParser.SlotFillContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#slotFill}.
	 * @param ctx the parse tree
	 */
	void exitSlotFill(XrackParser.SlotFillContext ctx);
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
	 * Enter a parse tree produced by {@link XrackParser#letStatement}.
	 * @param ctx the parse tree
	 */
	void enterLetStatement(XrackParser.LetStatementContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#letStatement}.
	 * @param ctx the parse tree
	 */
	void exitLetStatement(XrackParser.LetStatementContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#statementBlock}.
	 * @param ctx the parse tree
	 */
	void enterStatementBlock(XrackParser.StatementBlockContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#statementBlock}.
	 * @param ctx the parse tree
	 */
	void exitStatementBlock(XrackParser.StatementBlockContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#ifStatement}.
	 * @param ctx the parse tree
	 */
	void enterIfStatement(XrackParser.IfStatementContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#ifStatement}.
	 * @param ctx the parse tree
	 */
	void exitIfStatement(XrackParser.IfStatementContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#forStatement}.
	 * @param ctx the parse tree
	 */
	void enterForStatement(XrackParser.ForStatementContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#forStatement}.
	 * @param ctx the parse tree
	 */
	void exitForStatement(XrackParser.ForStatementContext ctx);
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
	 * Enter a parse tree produced by {@link XrackParser#iteration}.
	 * @param ctx the parse tree
	 */
	void enterIteration(XrackParser.IterationContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#iteration}.
	 * @param ctx the parse tree
	 */
	void exitIteration(XrackParser.IterationContext ctx);
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
	 * Enter a parse tree produced by {@link XrackParser#enumValue}.
	 * @param ctx the parse tree
	 */
	void enterEnumValue(XrackParser.EnumValueContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#enumValue}.
	 * @param ctx the parse tree
	 */
	void exitEnumValue(XrackParser.EnumValueContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#enumArguments}.
	 * @param ctx the parse tree
	 */
	void enterEnumArguments(XrackParser.EnumArgumentsContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#enumArguments}.
	 * @param ctx the parse tree
	 */
	void exitEnumArguments(XrackParser.EnumArgumentsContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#matchExpression}.
	 * @param ctx the parse tree
	 */
	void enterMatchExpression(XrackParser.MatchExpressionContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#matchExpression}.
	 * @param ctx the parse tree
	 */
	void exitMatchExpression(XrackParser.MatchExpressionContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#matchArm}.
	 * @param ctx the parse tree
	 */
	void enterMatchArm(XrackParser.MatchArmContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#matchArm}.
	 * @param ctx the parse tree
	 */
	void exitMatchArm(XrackParser.MatchArmContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#enumPattern}.
	 * @param ctx the parse tree
	 */
	void enterEnumPattern(XrackParser.EnumPatternContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#enumPattern}.
	 * @param ctx the parse tree
	 */
	void exitEnumPattern(XrackParser.EnumPatternContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#enumPatternFields}.
	 * @param ctx the parse tree
	 */
	void enterEnumPatternFields(XrackParser.EnumPatternFieldsContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#enumPatternFields}.
	 * @param ctx the parse tree
	 */
	void exitEnumPatternFields(XrackParser.EnumPatternFieldsContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#namedPatternField}.
	 * @param ctx the parse tree
	 */
	void enterNamedPatternField(XrackParser.NamedPatternFieldContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#namedPatternField}.
	 * @param ctx the parse tree
	 */
	void exitNamedPatternField(XrackParser.NamedPatternFieldContext ctx);
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
	 * Enter a parse tree produced by {@link XrackParser#recordBase}.
	 * @param ctx the parse tree
	 */
	void enterRecordBase(XrackParser.RecordBaseContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#recordBase}.
	 * @param ctx the parse tree
	 */
	void exitRecordBase(XrackParser.RecordBaseContext ctx);
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
	 * Enter a parse tree produced by {@link XrackParser#meshUrl}.
	 * @param ctx the parse tree
	 */
	void enterMeshUrl(XrackParser.MeshUrlContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#meshUrl}.
	 * @param ctx the parse tree
	 */
	void exitMeshUrl(XrackParser.MeshUrlContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#environmentUrl}.
	 * @param ctx the parse tree
	 */
	void enterEnvironmentUrl(XrackParser.EnvironmentUrlContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#environmentUrl}.
	 * @param ctx the parse tree
	 */
	void exitEnvironmentUrl(XrackParser.EnvironmentUrlContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#textureUrl}.
	 * @param ctx the parse tree
	 */
	void enterTextureUrl(XrackParser.TextureUrlContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#textureUrl}.
	 * @param ctx the parse tree
	 */
	void exitTextureUrl(XrackParser.TextureUrlContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#shaderUrl}.
	 * @param ctx the parse tree
	 */
	void enterShaderUrl(XrackParser.ShaderUrlContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#shaderUrl}.
	 * @param ctx the parse tree
	 */
	void exitShaderUrl(XrackParser.ShaderUrlContext ctx);
	/**
	 * Enter a parse tree produced by {@link XrackParser#materialShaderUrl}.
	 * @param ctx the parse tree
	 */
	void enterMaterialShaderUrl(XrackParser.MaterialShaderUrlContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#materialShaderUrl}.
	 * @param ctx the parse tree
	 */
	void exitMaterialShaderUrl(XrackParser.MaterialShaderUrlContext ctx);
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
	 * Enter a parse tree produced by {@link XrackParser#translation}.
	 * @param ctx the parse tree
	 */
	void enterTranslation(XrackParser.TranslationContext ctx);
	/**
	 * Exit a parse tree produced by {@link XrackParser#translation}.
	 * @param ctx the parse tree
	 */
	void exitTranslation(XrackParser.TranslationContext ctx);
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