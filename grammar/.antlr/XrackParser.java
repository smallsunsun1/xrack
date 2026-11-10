// Generated from /Users/sunjiahe/coding/xrack/grammar/Xrack.g4 by ANTLR 4.13.1
import org.antlr.v4.runtime.atn.*;
import org.antlr.v4.runtime.dfa.DFA;
import org.antlr.v4.runtime.*;
import org.antlr.v4.runtime.misc.*;
import org.antlr.v4.runtime.tree.*;
import java.util.List;
import java.util.Iterator;
import java.util.ArrayList;

@SuppressWarnings({"all", "warnings", "unchecked", "unused", "cast", "CheckReturnValue"})
public class XrackParser extends Parser {
	static { RuntimeMetaData.checkVersion("4.13.1", RuntimeMetaData.VERSION); }

	protected static final DFA[] _decisionToDFA;
	protected static final PredictionContextCache _sharedContextCache =
		new PredictionContextCache();
	public static final int
		T__0=1, T__1=2, T__2=3, T__3=4, T__4=5, T__5=6, T__6=7, T__7=8, T__8=9, 
		T__9=10, T__10=11, T__11=12, T__12=13, T__13=14, T__14=15, T__15=16, T__16=17, 
		T__17=18, T__18=19, T__19=20, T__20=21, T__21=22, T__22=23, T__23=24, 
		T__24=25, T__25=26, T__26=27, T__27=28, T__28=29, T__29=30, T__30=31, 
		T__31=32, T__32=33, T__33=34, T__34=35, T__35=36, T__36=37, T__37=38, 
		T__38=39, T__39=40, T__40=41, T__41=42, T__42=43, T__43=44, T__44=45, 
		T__45=46, T__46=47, T__47=48, T__48=49, T__49=50, T__50=51, T__51=52, 
		T__52=53, T__53=54, T__54=55, T__55=56, T__56=57, T__57=58, T__58=59, 
		T__59=60, T__60=61, T__61=62, T__62=63, T__63=64, T__64=65, T__65=66, 
		T__66=67, T__67=68, T__68=69, T__69=70, T__70=71, T__71=72, T__72=73, 
		T__73=74, T__74=75, T__75=76, T__76=77, T__77=78, T__78=79, T__79=80, 
		T__80=81, UNIT=82, COLOR=83, NUMBER=84, STRING=85, IDENT=86, LINE_COMMENT=87, 
		BLOCK_COMMENT=88, WS=89;
	public static final int
		RULE_document = 0, RULE_importDecl = 1, RULE_fontImport = 2, RULE_importName = 3, 
		RULE_recordDecl = 4, RULE_recordField = 5, RULE_component = 6, RULE_member = 7, 
		RULE_animation = 8, RULE_animationBody = 9, RULE_animationField = 10, 
		RULE_stateBlock = 11, RULE_stateDecl = 12, RULE_stateBinding = 13, RULE_stateTransition = 14, 
		RULE_transitionAnimation = 15, RULE_property = 16, RULE_visibility = 17, 
		RULE_typeName = 18, RULE_callback = 19, RULE_parameter = 20, RULE_binding = 21, 
		RULE_handler = 22, RULE_statement = 23, RULE_assignmentOp = 24, RULE_element = 25, 
		RULE_conditionClause = 26, RULE_repeatClause = 27, RULE_expression = 28, 
		RULE_conditional = 29, RULE_logicalOr = 30, RULE_logicalAnd = 31, RULE_equality = 32, 
		RULE_comparison = 33, RULE_additive = 34, RULE_multiplicative = 35, RULE_unary = 36, 
		RULE_primary = 37, RULE_call = 38, RULE_arrayLiteral = 39, RULE_recordLiteral = 40, 
		RULE_recordValue = 41, RULE_imageUrl = 42, RULE_imageSlice = 43, RULE_imageEdge = 44, 
		RULE_markdown = 45, RULE_gradient = 46, RULE_gradientAngle = 47, RULE_gradientStop = 48, 
		RULE_reference = 49, RULE_identifier = 50;
	private static String[] makeRuleNames() {
		return new String[] {
			"document", "importDecl", "fontImport", "importName", "recordDecl", "recordField", 
			"component", "member", "animation", "animationBody", "animationField", 
			"stateBlock", "stateDecl", "stateBinding", "stateTransition", "transitionAnimation", 
			"property", "visibility", "typeName", "callback", "parameter", "binding", 
			"handler", "statement", "assignmentOp", "element", "conditionClause", 
			"repeatClause", "expression", "conditional", "logicalOr", "logicalAnd", 
			"equality", "comparison", "additive", "multiplicative", "unary", "primary", 
			"call", "arrayLiteral", "recordLiteral", "recordValue", "imageUrl", "imageSlice", 
			"imageEdge", "markdown", "gradient", "gradientAngle", "gradientStop", 
			"reference", "identifier"
		};
	}
	public static final String[] ruleNames = makeRuleNames();

	private static String[] makeLiteralNames() {
		return new String[] {
			null, "'import'", "'{'", "','", "'}'", "'from'", "';'", "'as'", "'export'", 
			"'struct'", "':'", "'component'", "'inherits'", "'animate'", "'duration'", 
			"'ms'", "'states'", "'['", "']'", "'when'", "'in'", "'out'", "'in-out'", 
			"'*'", "'property'", "'<'", "'>'", "'<=>'", "'private'", "'pure'", "'callback'", 
			"'('", "')'", "'->'", "'=>'", "'='", "'+='", "'-='", "':='", "'if'", 
			"'for'", "'?'", "'||'", "'&&'", "'=='", "'!='", "'<='", "'>='", "'+'", 
			"'-'", "'/'", "'%'", "'!'", "'true'", "'false'", "'@image-url'", "'@markdown'", 
			"'@linear-gradient'", "'@radial-gradient'", "'circle'", "'deg'", "'root'", 
			"'self'", "'.'", "'int'", "'float'", "'bool'", "'string'", "'length'", 
			"'color'", "'brush'", "'image'", "'ImageFit'", "'LayoutAlignment'", "'CrossAxisAlignment'", 
			"'Orientation'", "'DialInputMode'", "'ScrollBarPolicy'", "'TextWrap'", 
			"'DialogRole'", "'DrawerEdge'", "'SelectionTarget'", "'px'"
		};
	}
	private static final String[] _LITERAL_NAMES = makeLiteralNames();
	private static String[] makeSymbolicNames() {
		return new String[] {
			null, null, null, null, null, null, null, null, null, null, null, null, 
			null, null, null, null, null, null, null, null, null, null, null, null, 
			null, null, null, null, null, null, null, null, null, null, null, null, 
			null, null, null, null, null, null, null, null, null, null, null, null, 
			null, null, null, null, null, null, null, null, null, null, null, null, 
			null, null, null, null, null, null, null, null, null, null, null, null, 
			null, null, null, null, null, null, null, null, null, null, "UNIT", "COLOR", 
			"NUMBER", "STRING", "IDENT", "LINE_COMMENT", "BLOCK_COMMENT", "WS"
		};
	}
	private static final String[] _SYMBOLIC_NAMES = makeSymbolicNames();
	public static final Vocabulary VOCABULARY = new VocabularyImpl(_LITERAL_NAMES, _SYMBOLIC_NAMES);

	/**
	 * @deprecated Use {@link #VOCABULARY} instead.
	 */
	@Deprecated
	public static final String[] tokenNames;
	static {
		tokenNames = new String[_SYMBOLIC_NAMES.length];
		for (int i = 0; i < tokenNames.length; i++) {
			tokenNames[i] = VOCABULARY.getLiteralName(i);
			if (tokenNames[i] == null) {
				tokenNames[i] = VOCABULARY.getSymbolicName(i);
			}

			if (tokenNames[i] == null) {
				tokenNames[i] = "<INVALID>";
			}
		}
	}

	@Override
	@Deprecated
	public String[] getTokenNames() {
		return tokenNames;
	}

	@Override

	public Vocabulary getVocabulary() {
		return VOCABULARY;
	}

	@Override
	public String getGrammarFileName() { return "Xrack.g4"; }

	@Override
	public String[] getRuleNames() { return ruleNames; }

	@Override
	public String getSerializedATN() { return _serializedATN; }

	@Override
	public ATN getATN() { return _ATN; }

	public XrackParser(TokenStream input) {
		super(input);
		_interp = new ParserATNSimulator(this,_ATN,_decisionToDFA,_sharedContextCache);
	}

	@SuppressWarnings("CheckReturnValue")
	public static class DocumentContext extends ParserRuleContext {
		public TerminalNode EOF() { return getToken(XrackParser.EOF, 0); }
		public List<ImportDeclContext> importDecl() {
			return getRuleContexts(ImportDeclContext.class);
		}
		public ImportDeclContext importDecl(int i) {
			return getRuleContext(ImportDeclContext.class,i);
		}
		public List<FontImportContext> fontImport() {
			return getRuleContexts(FontImportContext.class);
		}
		public FontImportContext fontImport(int i) {
			return getRuleContext(FontImportContext.class,i);
		}
		public List<RecordDeclContext> recordDecl() {
			return getRuleContexts(RecordDeclContext.class);
		}
		public RecordDeclContext recordDecl(int i) {
			return getRuleContext(RecordDeclContext.class,i);
		}
		public List<ComponentContext> component() {
			return getRuleContexts(ComponentContext.class);
		}
		public ComponentContext component(int i) {
			return getRuleContext(ComponentContext.class,i);
		}
		public DocumentContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_document; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterDocument(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitDocument(this);
		}
	}

	public final DocumentContext document() throws RecognitionException {
		DocumentContext _localctx = new DocumentContext(_ctx, getState());
		enterRule(_localctx, 0, RULE_document);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(106);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while (_la==T__0) {
				{
				setState(104);
				_errHandler.sync(this);
				switch ( getInterpreter().adaptivePredict(_input,0,_ctx) ) {
				case 1:
					{
					setState(102);
					importDecl();
					}
					break;
				case 2:
					{
					setState(103);
					fontImport();
					}
					break;
				}
				}
				setState(108);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			setState(111); 
			_errHandler.sync(this);
			_la = _input.LA(1);
			do {
				{
				setState(111);
				_errHandler.sync(this);
				switch ( getInterpreter().adaptivePredict(_input,2,_ctx) ) {
				case 1:
					{
					setState(109);
					recordDecl();
					}
					break;
				case 2:
					{
					setState(110);
					component();
					}
					break;
				}
				}
				setState(113); 
				_errHandler.sync(this);
				_la = _input.LA(1);
			} while ( (((_la) & ~0x3f) == 0 && ((1L << _la) & 2816L) != 0) );
			setState(115);
			match(EOF);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ImportDeclContext extends ParserRuleContext {
		public List<ImportNameContext> importName() {
			return getRuleContexts(ImportNameContext.class);
		}
		public ImportNameContext importName(int i) {
			return getRuleContext(ImportNameContext.class,i);
		}
		public TerminalNode STRING() { return getToken(XrackParser.STRING, 0); }
		public ImportDeclContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_importDecl; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterImportDecl(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitImportDecl(this);
		}
	}

	public final ImportDeclContext importDecl() throws RecognitionException {
		ImportDeclContext _localctx = new ImportDeclContext(_ctx, getState());
		enterRule(_localctx, 2, RULE_importDecl);
		int _la;
		try {
			int _alt;
			enterOuterAlt(_localctx, 1);
			{
			setState(117);
			match(T__0);
			setState(118);
			match(T__1);
			setState(119);
			importName();
			setState(124);
			_errHandler.sync(this);
			_alt = getInterpreter().adaptivePredict(_input,4,_ctx);
			while ( _alt!=2 && _alt!=org.antlr.v4.runtime.atn.ATN.INVALID_ALT_NUMBER ) {
				if ( _alt==1 ) {
					{
					{
					setState(120);
					match(T__2);
					setState(121);
					importName();
					}
					} 
				}
				setState(126);
				_errHandler.sync(this);
				_alt = getInterpreter().adaptivePredict(_input,4,_ctx);
			}
			setState(128);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__2) {
				{
				setState(127);
				match(T__2);
				}
			}

			setState(130);
			match(T__3);
			setState(131);
			match(T__4);
			setState(132);
			match(STRING);
			setState(133);
			match(T__5);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class FontImportContext extends ParserRuleContext {
		public TerminalNode STRING() { return getToken(XrackParser.STRING, 0); }
		public FontImportContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_fontImport; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterFontImport(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitFontImport(this);
		}
	}

	public final FontImportContext fontImport() throws RecognitionException {
		FontImportContext _localctx = new FontImportContext(_ctx, getState());
		enterRule(_localctx, 4, RULE_fontImport);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(135);
			match(T__0);
			setState(136);
			match(STRING);
			setState(137);
			match(T__5);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ImportNameContext extends ParserRuleContext {
		public List<IdentifierContext> identifier() {
			return getRuleContexts(IdentifierContext.class);
		}
		public IdentifierContext identifier(int i) {
			return getRuleContext(IdentifierContext.class,i);
		}
		public ImportNameContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_importName; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterImportName(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitImportName(this);
		}
	}

	public final ImportNameContext importName() throws RecognitionException {
		ImportNameContext _localctx = new ImportNameContext(_ctx, getState());
		enterRule(_localctx, 6, RULE_importName);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(139);
			identifier();
			setState(142);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__6) {
				{
				setState(140);
				match(T__6);
				setState(141);
				identifier();
				}
			}

			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class RecordDeclContext extends ParserRuleContext {
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public List<RecordFieldContext> recordField() {
			return getRuleContexts(RecordFieldContext.class);
		}
		public RecordFieldContext recordField(int i) {
			return getRuleContext(RecordFieldContext.class,i);
		}
		public RecordDeclContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_recordDecl; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterRecordDecl(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitRecordDecl(this);
		}
	}

	public final RecordDeclContext recordDecl() throws RecognitionException {
		RecordDeclContext _localctx = new RecordDeclContext(_ctx, getState());
		enterRule(_localctx, 8, RULE_recordDecl);
		int _la;
		try {
			int _alt;
			enterOuterAlt(_localctx, 1);
			{
			setState(145);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__7) {
				{
				setState(144);
				match(T__7);
				}
			}

			setState(147);
			match(T__8);
			setState(148);
			identifier();
			setState(149);
			match(T__1);
			setState(150);
			recordField();
			setState(155);
			_errHandler.sync(this);
			_alt = getInterpreter().adaptivePredict(_input,8,_ctx);
			while ( _alt!=2 && _alt!=org.antlr.v4.runtime.atn.ATN.INVALID_ALT_NUMBER ) {
				if ( _alt==1 ) {
					{
					{
					setState(151);
					match(T__2);
					setState(152);
					recordField();
					}
					} 
				}
				setState(157);
				_errHandler.sync(this);
				_alt = getInterpreter().adaptivePredict(_input,8,_ctx);
			}
			setState(159);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__2) {
				{
				setState(158);
				match(T__2);
				}
			}

			setState(161);
			match(T__3);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class RecordFieldContext extends ParserRuleContext {
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public TypeNameContext typeName() {
			return getRuleContext(TypeNameContext.class,0);
		}
		public RecordFieldContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_recordField; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterRecordField(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitRecordField(this);
		}
	}

	public final RecordFieldContext recordField() throws RecognitionException {
		RecordFieldContext _localctx = new RecordFieldContext(_ctx, getState());
		enterRule(_localctx, 10, RULE_recordField);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(163);
			identifier();
			setState(164);
			match(T__9);
			setState(165);
			typeName();
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ComponentContext extends ParserRuleContext {
		public List<IdentifierContext> identifier() {
			return getRuleContexts(IdentifierContext.class);
		}
		public IdentifierContext identifier(int i) {
			return getRuleContext(IdentifierContext.class,i);
		}
		public List<MemberContext> member() {
			return getRuleContexts(MemberContext.class);
		}
		public MemberContext member(int i) {
			return getRuleContext(MemberContext.class,i);
		}
		public ComponentContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_component; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterComponent(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitComponent(this);
		}
	}

	public final ComponentContext component() throws RecognitionException {
		ComponentContext _localctx = new ComponentContext(_ctx, getState());
		enterRule(_localctx, 12, RULE_component);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(168);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__7) {
				{
				setState(167);
				match(T__7);
				}
			}

			setState(170);
			match(T__10);
			setState(171);
			identifier();
			setState(172);
			match(T__11);
			setState(173);
			identifier();
			setState(174);
			match(T__1);
			setState(178);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while ((((_la) & ~0x3f) == 0 && ((1L << _la) & 1729383908081525408L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 4456447L) != 0)) {
				{
				{
				setState(175);
				member();
				}
				}
				setState(180);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			setState(181);
			match(T__3);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class MemberContext extends ParserRuleContext {
		public PropertyContext property() {
			return getRuleContext(PropertyContext.class,0);
		}
		public CallbackContext callback() {
			return getRuleContext(CallbackContext.class,0);
		}
		public BindingContext binding() {
			return getRuleContext(BindingContext.class,0);
		}
		public HandlerContext handler() {
			return getRuleContext(HandlerContext.class,0);
		}
		public ElementContext element() {
			return getRuleContext(ElementContext.class,0);
		}
		public AnimationContext animation() {
			return getRuleContext(AnimationContext.class,0);
		}
		public StateBlockContext stateBlock() {
			return getRuleContext(StateBlockContext.class,0);
		}
		public MemberContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_member; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterMember(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitMember(this);
		}
	}

	public final MemberContext member() throws RecognitionException {
		MemberContext _localctx = new MemberContext(_ctx, getState());
		enterRule(_localctx, 14, RULE_member);
		try {
			setState(190);
			_errHandler.sync(this);
			switch ( getInterpreter().adaptivePredict(_input,12,_ctx) ) {
			case 1:
				enterOuterAlt(_localctx, 1);
				{
				setState(183);
				property();
				}
				break;
			case 2:
				enterOuterAlt(_localctx, 2);
				{
				setState(184);
				callback();
				}
				break;
			case 3:
				enterOuterAlt(_localctx, 3);
				{
				setState(185);
				binding();
				}
				break;
			case 4:
				enterOuterAlt(_localctx, 4);
				{
				setState(186);
				handler();
				}
				break;
			case 5:
				enterOuterAlt(_localctx, 5);
				{
				setState(187);
				element();
				}
				break;
			case 6:
				enterOuterAlt(_localctx, 6);
				{
				setState(188);
				animation();
				}
				break;
			case 7:
				enterOuterAlt(_localctx, 7);
				{
				setState(189);
				stateBlock();
				}
				break;
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class AnimationContext extends ParserRuleContext {
		public List<IdentifierContext> identifier() {
			return getRuleContexts(IdentifierContext.class);
		}
		public IdentifierContext identifier(int i) {
			return getRuleContext(IdentifierContext.class,i);
		}
		public AnimationBodyContext animationBody() {
			return getRuleContext(AnimationBodyContext.class,0);
		}
		public AnimationContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_animation; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterAnimation(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitAnimation(this);
		}
	}

	public final AnimationContext animation() throws RecognitionException {
		AnimationContext _localctx = new AnimationContext(_ctx, getState());
		enterRule(_localctx, 16, RULE_animation);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(192);
			match(T__12);
			setState(193);
			identifier();
			setState(198);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while (_la==T__2) {
				{
				{
				setState(194);
				match(T__2);
				setState(195);
				identifier();
				}
				}
				setState(200);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			setState(201);
			animationBody();
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class AnimationBodyContext extends ParserRuleContext {
		public List<AnimationFieldContext> animationField() {
			return getRuleContexts(AnimationFieldContext.class);
		}
		public AnimationFieldContext animationField(int i) {
			return getRuleContext(AnimationFieldContext.class,i);
		}
		public AnimationBodyContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_animationBody; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterAnimationBody(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitAnimationBody(this);
		}
	}

	public final AnimationBodyContext animationBody() throws RecognitionException {
		AnimationBodyContext _localctx = new AnimationBodyContext(_ctx, getState());
		enterRule(_localctx, 18, RULE_animationBody);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(203);
			match(T__1);
			setState(207);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while ((((_la) & ~0x3f) == 0 && ((1L << _la) & 1729382257447781024L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 4456447L) != 0)) {
				{
				{
				setState(204);
				animationField();
				}
				}
				setState(209);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			setState(210);
			match(T__3);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class AnimationFieldContext extends ParserRuleContext {
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public TerminalNode NUMBER() { return getToken(XrackParser.NUMBER, 0); }
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public AnimationFieldContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_animationField; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterAnimationField(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitAnimationField(this);
		}
	}

	public final AnimationFieldContext animationField() throws RecognitionException {
		AnimationFieldContext _localctx = new AnimationFieldContext(_ctx, getState());
		enterRule(_localctx, 20, RULE_animationField);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(214);
			_errHandler.sync(this);
			switch ( getInterpreter().adaptivePredict(_input,15,_ctx) ) {
			case 1:
				{
				setState(212);
				match(T__13);
				}
				break;
			case 2:
				{
				setState(213);
				identifier();
				}
				break;
			}
			setState(216);
			match(T__9);
			setState(220);
			_errHandler.sync(this);
			switch ( getInterpreter().adaptivePredict(_input,16,_ctx) ) {
			case 1:
				{
				setState(217);
				match(NUMBER);
				setState(218);
				match(T__14);
				}
				break;
			case 2:
				{
				setState(219);
				expression();
				}
				break;
			}
			setState(222);
			match(T__5);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class StateBlockContext extends ParserRuleContext {
		public List<StateDeclContext> stateDecl() {
			return getRuleContexts(StateDeclContext.class);
		}
		public StateDeclContext stateDecl(int i) {
			return getRuleContext(StateDeclContext.class,i);
		}
		public StateBlockContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_stateBlock; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterStateBlock(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitStateBlock(this);
		}
	}

	public final StateBlockContext stateBlock() throws RecognitionException {
		StateBlockContext _localctx = new StateBlockContext(_ctx, getState());
		enterRule(_localctx, 22, RULE_stateBlock);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(224);
			match(T__15);
			setState(225);
			match(T__16);
			setState(229);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while ((((_la) & ~0x3f) == 0 && ((1L << _la) & 1729382257447781024L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 4456447L) != 0)) {
				{
				{
				setState(226);
				stateDecl();
				}
				}
				setState(231);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			setState(232);
			match(T__17);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class StateDeclContext extends ParserRuleContext {
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public List<StateBindingContext> stateBinding() {
			return getRuleContexts(StateBindingContext.class);
		}
		public StateBindingContext stateBinding(int i) {
			return getRuleContext(StateBindingContext.class,i);
		}
		public List<StateTransitionContext> stateTransition() {
			return getRuleContexts(StateTransitionContext.class);
		}
		public StateTransitionContext stateTransition(int i) {
			return getRuleContext(StateTransitionContext.class,i);
		}
		public StateDeclContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_stateDecl; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterStateDecl(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitStateDecl(this);
		}
	}

	public final StateDeclContext stateDecl() throws RecognitionException {
		StateDeclContext _localctx = new StateDeclContext(_ctx, getState());
		enterRule(_localctx, 24, RULE_stateDecl);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(234);
			identifier();
			setState(237);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__18) {
				{
				setState(235);
				match(T__18);
				setState(236);
				expression();
				}
			}

			setState(239);
			match(T__9);
			setState(240);
			match(T__1);
			setState(245);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while ((((_la) & ~0x3f) == 0 && ((1L << _la) & 8646911285096202912L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 4456447L) != 0)) {
				{
				setState(243);
				_errHandler.sync(this);
				switch (_input.LA(1)) {
				case T__4:
				case T__6:
				case T__8:
				case T__13:
				case T__14:
				case T__15:
				case T__18:
				case T__28:
				case T__58:
				case T__59:
				case T__60:
				case T__61:
				case T__63:
				case T__64:
				case T__65:
				case T__66:
				case T__67:
				case T__68:
				case T__69:
				case T__70:
				case T__71:
				case T__72:
				case T__73:
				case T__74:
				case T__75:
				case T__76:
				case T__77:
				case T__78:
				case T__79:
				case T__80:
				case IDENT:
					{
					setState(241);
					stateBinding();
					}
					break;
				case T__19:
				case T__20:
				case T__21:
					{
					setState(242);
					stateTransition();
					}
					break;
				default:
					throw new NoViableAltException(this);
				}
				}
				setState(247);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			setState(248);
			match(T__3);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class StateBindingContext extends ParserRuleContext {
		public ReferenceContext reference() {
			return getRuleContext(ReferenceContext.class,0);
		}
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public StateBindingContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_stateBinding; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterStateBinding(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitStateBinding(this);
		}
	}

	public final StateBindingContext stateBinding() throws RecognitionException {
		StateBindingContext _localctx = new StateBindingContext(_ctx, getState());
		enterRule(_localctx, 26, RULE_stateBinding);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(250);
			reference();
			setState(251);
			match(T__9);
			setState(252);
			expression();
			setState(253);
			match(T__5);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class StateTransitionContext extends ParserRuleContext {
		public List<TransitionAnimationContext> transitionAnimation() {
			return getRuleContexts(TransitionAnimationContext.class);
		}
		public TransitionAnimationContext transitionAnimation(int i) {
			return getRuleContext(TransitionAnimationContext.class,i);
		}
		public StateTransitionContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_stateTransition; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterStateTransition(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitStateTransition(this);
		}
	}

	public final StateTransitionContext stateTransition() throws RecognitionException {
		StateTransitionContext _localctx = new StateTransitionContext(_ctx, getState());
		enterRule(_localctx, 28, RULE_stateTransition);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(255);
			_la = _input.LA(1);
			if ( !((((_la) & ~0x3f) == 0 && ((1L << _la) & 7340032L) != 0)) ) {
			_errHandler.recoverInline(this);
			}
			else {
				if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
				_errHandler.reportMatch(this);
				consume();
			}
			setState(256);
			match(T__1);
			setState(260);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while (_la==T__12) {
				{
				{
				setState(257);
				transitionAnimation();
				}
				}
				setState(262);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			setState(263);
			match(T__3);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class TransitionAnimationContext extends ParserRuleContext {
		public AnimationBodyContext animationBody() {
			return getRuleContext(AnimationBodyContext.class,0);
		}
		public List<ReferenceContext> reference() {
			return getRuleContexts(ReferenceContext.class);
		}
		public ReferenceContext reference(int i) {
			return getRuleContext(ReferenceContext.class,i);
		}
		public TransitionAnimationContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_transitionAnimation; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterTransitionAnimation(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitTransitionAnimation(this);
		}
	}

	public final TransitionAnimationContext transitionAnimation() throws RecognitionException {
		TransitionAnimationContext _localctx = new TransitionAnimationContext(_ctx, getState());
		enterRule(_localctx, 30, RULE_transitionAnimation);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(265);
			match(T__12);
			setState(275);
			_errHandler.sync(this);
			switch (_input.LA(1)) {
			case T__4:
			case T__6:
			case T__8:
			case T__13:
			case T__14:
			case T__15:
			case T__18:
			case T__28:
			case T__58:
			case T__59:
			case T__60:
			case T__61:
			case T__63:
			case T__64:
			case T__65:
			case T__66:
			case T__67:
			case T__68:
			case T__69:
			case T__70:
			case T__71:
			case T__72:
			case T__73:
			case T__74:
			case T__75:
			case T__76:
			case T__77:
			case T__78:
			case T__79:
			case T__80:
			case IDENT:
				{
				setState(266);
				reference();
				setState(271);
				_errHandler.sync(this);
				_la = _input.LA(1);
				while (_la==T__2) {
					{
					{
					setState(267);
					match(T__2);
					setState(268);
					reference();
					}
					}
					setState(273);
					_errHandler.sync(this);
					_la = _input.LA(1);
				}
				}
				break;
			case T__22:
				{
				setState(274);
				match(T__22);
				}
				break;
			default:
				throw new NoViableAltException(this);
			}
			setState(277);
			animationBody();
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class PropertyContext extends ParserRuleContext {
		public TypeNameContext typeName() {
			return getRuleContext(TypeNameContext.class,0);
		}
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public VisibilityContext visibility() {
			return getRuleContext(VisibilityContext.class,0);
		}
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public ReferenceContext reference() {
			return getRuleContext(ReferenceContext.class,0);
		}
		public PropertyContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_property; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterProperty(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitProperty(this);
		}
	}

	public final PropertyContext property() throws RecognitionException {
		PropertyContext _localctx = new PropertyContext(_ctx, getState());
		enterRule(_localctx, 32, RULE_property);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(280);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if ((((_la) & ~0x3f) == 0 && ((1L << _la) & 275775488L) != 0)) {
				{
				setState(279);
				visibility();
				}
			}

			setState(282);
			match(T__23);
			setState(283);
			match(T__24);
			setState(284);
			typeName();
			setState(285);
			match(T__25);
			setState(286);
			identifier();
			setState(291);
			_errHandler.sync(this);
			switch (_input.LA(1)) {
			case T__9:
				{
				setState(287);
				match(T__9);
				setState(288);
				expression();
				}
				break;
			case T__26:
				{
				setState(289);
				match(T__26);
				setState(290);
				reference();
				}
				break;
			case T__5:
				break;
			default:
				break;
			}
			setState(293);
			match(T__5);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class VisibilityContext extends ParserRuleContext {
		public VisibilityContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_visibility; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterVisibility(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitVisibility(this);
		}
	}

	public final VisibilityContext visibility() throws RecognitionException {
		VisibilityContext _localctx = new VisibilityContext(_ctx, getState());
		enterRule(_localctx, 34, RULE_visibility);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(295);
			_la = _input.LA(1);
			if ( !((((_la) & ~0x3f) == 0 && ((1L << _la) & 275775488L) != 0)) ) {
			_errHandler.recoverInline(this);
			}
			else {
				if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
				_errHandler.reportMatch(this);
				consume();
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class TypeNameContext extends ParserRuleContext {
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public TypeNameContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_typeName; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterTypeName(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitTypeName(this);
		}
	}

	public final TypeNameContext typeName() throws RecognitionException {
		TypeNameContext _localctx = new TypeNameContext(_ctx, getState());
		enterRule(_localctx, 36, RULE_typeName);
		try {
			setState(302);
			_errHandler.sync(this);
			switch (_input.LA(1)) {
			case T__4:
			case T__6:
			case T__8:
			case T__13:
			case T__14:
			case T__15:
			case T__18:
			case T__28:
			case T__58:
			case T__59:
			case T__63:
			case T__64:
			case T__65:
			case T__66:
			case T__67:
			case T__68:
			case T__69:
			case T__70:
			case T__71:
			case T__72:
			case T__73:
			case T__74:
			case T__75:
			case T__76:
			case T__77:
			case T__78:
			case T__79:
			case T__80:
			case IDENT:
				enterOuterAlt(_localctx, 1);
				{
				setState(297);
				identifier();
				}
				break;
			case T__16:
				enterOuterAlt(_localctx, 2);
				{
				setState(298);
				match(T__16);
				setState(299);
				identifier();
				setState(300);
				match(T__17);
				}
				break;
			default:
				throw new NoViableAltException(this);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class CallbackContext extends ParserRuleContext {
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public ReferenceContext reference() {
			return getRuleContext(ReferenceContext.class,0);
		}
		public List<ParameterContext> parameter() {
			return getRuleContexts(ParameterContext.class);
		}
		public ParameterContext parameter(int i) {
			return getRuleContext(ParameterContext.class,i);
		}
		public TypeNameContext typeName() {
			return getRuleContext(TypeNameContext.class,0);
		}
		public CallbackContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_callback; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterCallback(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitCallback(this);
		}
	}

	public final CallbackContext callback() throws RecognitionException {
		CallbackContext _localctx = new CallbackContext(_ctx, getState());
		enterRule(_localctx, 38, RULE_callback);
		int _la;
		try {
			int _alt;
			enterOuterAlt(_localctx, 1);
			{
			setState(305);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__28) {
				{
				setState(304);
				match(T__28);
				}
			}

			setState(307);
			match(T__29);
			setState(308);
			identifier();
			setState(330);
			_errHandler.sync(this);
			switch (_input.LA(1)) {
			case T__26:
				{
				setState(309);
				match(T__26);
				setState(310);
				reference();
				}
				break;
			case T__30:
				{
				setState(311);
				match(T__30);
				setState(323);
				_errHandler.sync(this);
				_la = _input.LA(1);
				if ((((_la) & ~0x3f) == 0 && ((1L << _la) & 1729382257447912096L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 4456447L) != 0)) {
					{
					setState(312);
					parameter();
					setState(317);
					_errHandler.sync(this);
					_alt = getInterpreter().adaptivePredict(_input,28,_ctx);
					while ( _alt!=2 && _alt!=org.antlr.v4.runtime.atn.ATN.INVALID_ALT_NUMBER ) {
						if ( _alt==1 ) {
							{
							{
							setState(313);
							match(T__2);
							setState(314);
							parameter();
							}
							} 
						}
						setState(319);
						_errHandler.sync(this);
						_alt = getInterpreter().adaptivePredict(_input,28,_ctx);
					}
					setState(321);
					_errHandler.sync(this);
					_la = _input.LA(1);
					if (_la==T__2) {
						{
						setState(320);
						match(T__2);
						}
					}

					}
				}

				setState(325);
				match(T__31);
				setState(328);
				_errHandler.sync(this);
				_la = _input.LA(1);
				if (_la==T__32) {
					{
					setState(326);
					match(T__32);
					setState(327);
					typeName();
					}
				}

				}
				break;
			case T__5:
				break;
			default:
				break;
			}
			setState(332);
			match(T__5);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ParameterContext extends ParserRuleContext {
		public TypeNameContext typeName() {
			return getRuleContext(TypeNameContext.class,0);
		}
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public ParameterContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_parameter; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterParameter(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitParameter(this);
		}
	}

	public final ParameterContext parameter() throws RecognitionException {
		ParameterContext _localctx = new ParameterContext(_ctx, getState());
		enterRule(_localctx, 40, RULE_parameter);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(337);
			_errHandler.sync(this);
			switch ( getInterpreter().adaptivePredict(_input,33,_ctx) ) {
			case 1:
				{
				setState(334);
				identifier();
				setState(335);
				match(T__9);
				}
				break;
			}
			setState(339);
			typeName();
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class BindingContext extends ParserRuleContext {
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public ReferenceContext reference() {
			return getRuleContext(ReferenceContext.class,0);
		}
		public BindingContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_binding; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterBinding(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitBinding(this);
		}
	}

	public final BindingContext binding() throws RecognitionException {
		BindingContext _localctx = new BindingContext(_ctx, getState());
		enterRule(_localctx, 42, RULE_binding);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(341);
			identifier();
			setState(346);
			_errHandler.sync(this);
			switch (_input.LA(1)) {
			case T__9:
				{
				setState(342);
				match(T__9);
				setState(343);
				expression();
				}
				break;
			case T__26:
				{
				setState(344);
				match(T__26);
				setState(345);
				reference();
				}
				break;
			default:
				throw new NoViableAltException(this);
			}
			setState(348);
			match(T__5);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class HandlerContext extends ParserRuleContext {
		public List<IdentifierContext> identifier() {
			return getRuleContexts(IdentifierContext.class);
		}
		public IdentifierContext identifier(int i) {
			return getRuleContext(IdentifierContext.class,i);
		}
		public List<StatementContext> statement() {
			return getRuleContexts(StatementContext.class);
		}
		public StatementContext statement(int i) {
			return getRuleContext(StatementContext.class,i);
		}
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public HandlerContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_handler; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterHandler(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitHandler(this);
		}
	}

	public final HandlerContext handler() throws RecognitionException {
		HandlerContext _localctx = new HandlerContext(_ctx, getState());
		enterRule(_localctx, 44, RULE_handler);
		int _la;
		try {
			int _alt;
			enterOuterAlt(_localctx, 1);
			{
			setState(350);
			identifier();
			setState(363);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__30) {
				{
				setState(351);
				match(T__30);
				setState(360);
				_errHandler.sync(this);
				_la = _input.LA(1);
				if ((((_la) & ~0x3f) == 0 && ((1L << _la) & 1729382257447781024L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 4456447L) != 0)) {
					{
					setState(352);
					identifier();
					setState(357);
					_errHandler.sync(this);
					_la = _input.LA(1);
					while (_la==T__2) {
						{
						{
						setState(353);
						match(T__2);
						setState(354);
						identifier();
						}
						}
						setState(359);
						_errHandler.sync(this);
						_la = _input.LA(1);
					}
					}
				}

				setState(362);
				match(T__31);
				}
			}

			setState(365);
			match(T__33);
			setState(366);
			match(T__1);
			setState(370);
			_errHandler.sync(this);
			_alt = getInterpreter().adaptivePredict(_input,38,_ctx);
			while ( _alt!=2 && _alt!=org.antlr.v4.runtime.atn.ATN.INVALID_ALT_NUMBER ) {
				if ( _alt==1 ) {
					{
					{
					setState(367);
					statement();
					}
					} 
				}
				setState(372);
				_errHandler.sync(this);
				_alt = getInterpreter().adaptivePredict(_input,38,_ctx);
			}
			setState(377);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if ((((_la) & ~0x3f) == 0 && ((1L << _la) & 9219712864842662560L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 8126463L) != 0)) {
				{
				setState(373);
				expression();
				setState(375);
				_errHandler.sync(this);
				_la = _input.LA(1);
				if (_la==T__5) {
					{
					setState(374);
					match(T__5);
					}
				}

				}
			}

			setState(379);
			match(T__3);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class StatementContext extends ParserRuleContext {
		public ReferenceContext reference() {
			return getRuleContext(ReferenceContext.class,0);
		}
		public AssignmentOpContext assignmentOp() {
			return getRuleContext(AssignmentOpContext.class,0);
		}
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public CallContext call() {
			return getRuleContext(CallContext.class,0);
		}
		public StatementContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_statement; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterStatement(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitStatement(this);
		}
	}

	public final StatementContext statement() throws RecognitionException {
		StatementContext _localctx = new StatementContext(_ctx, getState());
		enterRule(_localctx, 46, RULE_statement);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(386);
			_errHandler.sync(this);
			switch ( getInterpreter().adaptivePredict(_input,41,_ctx) ) {
			case 1:
				{
				setState(381);
				reference();
				setState(382);
				assignmentOp();
				setState(383);
				expression();
				}
				break;
			case 2:
				{
				setState(385);
				call();
				}
				break;
			}
			setState(388);
			match(T__5);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class AssignmentOpContext extends ParserRuleContext {
		public AssignmentOpContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_assignmentOp; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterAssignmentOp(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitAssignmentOp(this);
		}
	}

	public final AssignmentOpContext assignmentOp() throws RecognitionException {
		AssignmentOpContext _localctx = new AssignmentOpContext(_ctx, getState());
		enterRule(_localctx, 48, RULE_assignmentOp);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(390);
			_la = _input.LA(1);
			if ( !((((_la) & ~0x3f) == 0 && ((1L << _la) & 240518168576L) != 0)) ) {
			_errHandler.recoverInline(this);
			}
			else {
				if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
				_errHandler.reportMatch(this);
				consume();
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ElementContext extends ParserRuleContext {
		public List<IdentifierContext> identifier() {
			return getRuleContexts(IdentifierContext.class);
		}
		public IdentifierContext identifier(int i) {
			return getRuleContext(IdentifierContext.class,i);
		}
		public ConditionClauseContext conditionClause() {
			return getRuleContext(ConditionClauseContext.class,0);
		}
		public RepeatClauseContext repeatClause() {
			return getRuleContext(RepeatClauseContext.class,0);
		}
		public List<MemberContext> member() {
			return getRuleContexts(MemberContext.class);
		}
		public MemberContext member(int i) {
			return getRuleContext(MemberContext.class,i);
		}
		public ElementContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_element; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterElement(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitElement(this);
		}
	}

	public final ElementContext element() throws RecognitionException {
		ElementContext _localctx = new ElementContext(_ctx, getState());
		enterRule(_localctx, 50, RULE_element);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(394);
			_errHandler.sync(this);
			switch (_input.LA(1)) {
			case T__38:
				{
				setState(392);
				conditionClause();
				}
				break;
			case T__39:
				{
				setState(393);
				repeatClause();
				}
				break;
			case T__4:
			case T__6:
			case T__8:
			case T__13:
			case T__14:
			case T__15:
			case T__18:
			case T__28:
			case T__58:
			case T__59:
			case T__63:
			case T__64:
			case T__65:
			case T__66:
			case T__67:
			case T__68:
			case T__69:
			case T__70:
			case T__71:
			case T__72:
			case T__73:
			case T__74:
			case T__75:
			case T__76:
			case T__77:
			case T__78:
			case T__79:
			case T__80:
			case IDENT:
				break;
			default:
				break;
			}
			setState(399);
			_errHandler.sync(this);
			switch ( getInterpreter().adaptivePredict(_input,43,_ctx) ) {
			case 1:
				{
				setState(396);
				identifier();
				setState(397);
				match(T__37);
				}
				break;
			}
			setState(401);
			identifier();
			setState(402);
			match(T__1);
			setState(406);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while ((((_la) & ~0x3f) == 0 && ((1L << _la) & 1729383908081525408L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 4456447L) != 0)) {
				{
				{
				setState(403);
				member();
				}
				}
				setState(408);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			setState(409);
			match(T__3);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ConditionClauseContext extends ParserRuleContext {
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public ConditionClauseContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_conditionClause; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterConditionClause(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitConditionClause(this);
		}
	}

	public final ConditionClauseContext conditionClause() throws RecognitionException {
		ConditionClauseContext _localctx = new ConditionClauseContext(_ctx, getState());
		enterRule(_localctx, 52, RULE_conditionClause);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(411);
			match(T__38);
			setState(412);
			expression();
			setState(413);
			match(T__9);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class RepeatClauseContext extends ParserRuleContext {
		public List<IdentifierContext> identifier() {
			return getRuleContexts(IdentifierContext.class);
		}
		public IdentifierContext identifier(int i) {
			return getRuleContext(IdentifierContext.class,i);
		}
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public RepeatClauseContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_repeatClause; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterRepeatClause(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitRepeatClause(this);
		}
	}

	public final RepeatClauseContext repeatClause() throws RecognitionException {
		RepeatClauseContext _localctx = new RepeatClauseContext(_ctx, getState());
		enterRule(_localctx, 54, RULE_repeatClause);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(415);
			match(T__39);
			setState(416);
			identifier();
			setState(421);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__16) {
				{
				setState(417);
				match(T__16);
				setState(418);
				identifier();
				setState(419);
				match(T__17);
				}
			}

			setState(423);
			match(T__19);
			setState(424);
			expression();
			setState(425);
			match(T__9);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ExpressionContext extends ParserRuleContext {
		public ConditionalContext conditional() {
			return getRuleContext(ConditionalContext.class,0);
		}
		public ExpressionContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_expression; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterExpression(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitExpression(this);
		}
	}

	public final ExpressionContext expression() throws RecognitionException {
		ExpressionContext _localctx = new ExpressionContext(_ctx, getState());
		enterRule(_localctx, 56, RULE_expression);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(427);
			conditional();
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ConditionalContext extends ParserRuleContext {
		public LogicalOrContext logicalOr() {
			return getRuleContext(LogicalOrContext.class,0);
		}
		public List<ExpressionContext> expression() {
			return getRuleContexts(ExpressionContext.class);
		}
		public ExpressionContext expression(int i) {
			return getRuleContext(ExpressionContext.class,i);
		}
		public ConditionalContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_conditional; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterConditional(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitConditional(this);
		}
	}

	public final ConditionalContext conditional() throws RecognitionException {
		ConditionalContext _localctx = new ConditionalContext(_ctx, getState());
		enterRule(_localctx, 58, RULE_conditional);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(429);
			logicalOr();
			setState(435);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__40) {
				{
				setState(430);
				match(T__40);
				setState(431);
				expression();
				setState(432);
				match(T__9);
				setState(433);
				expression();
				}
			}

			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class LogicalOrContext extends ParserRuleContext {
		public List<LogicalAndContext> logicalAnd() {
			return getRuleContexts(LogicalAndContext.class);
		}
		public LogicalAndContext logicalAnd(int i) {
			return getRuleContext(LogicalAndContext.class,i);
		}
		public LogicalOrContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_logicalOr; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterLogicalOr(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitLogicalOr(this);
		}
	}

	public final LogicalOrContext logicalOr() throws RecognitionException {
		LogicalOrContext _localctx = new LogicalOrContext(_ctx, getState());
		enterRule(_localctx, 60, RULE_logicalOr);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(437);
			logicalAnd();
			setState(442);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while (_la==T__41) {
				{
				{
				setState(438);
				match(T__41);
				setState(439);
				logicalAnd();
				}
				}
				setState(444);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class LogicalAndContext extends ParserRuleContext {
		public List<EqualityContext> equality() {
			return getRuleContexts(EqualityContext.class);
		}
		public EqualityContext equality(int i) {
			return getRuleContext(EqualityContext.class,i);
		}
		public LogicalAndContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_logicalAnd; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterLogicalAnd(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitLogicalAnd(this);
		}
	}

	public final LogicalAndContext logicalAnd() throws RecognitionException {
		LogicalAndContext _localctx = new LogicalAndContext(_ctx, getState());
		enterRule(_localctx, 62, RULE_logicalAnd);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(445);
			equality();
			setState(450);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while (_la==T__42) {
				{
				{
				setState(446);
				match(T__42);
				setState(447);
				equality();
				}
				}
				setState(452);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class EqualityContext extends ParserRuleContext {
		public List<ComparisonContext> comparison() {
			return getRuleContexts(ComparisonContext.class);
		}
		public ComparisonContext comparison(int i) {
			return getRuleContext(ComparisonContext.class,i);
		}
		public EqualityContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_equality; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterEquality(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitEquality(this);
		}
	}

	public final EqualityContext equality() throws RecognitionException {
		EqualityContext _localctx = new EqualityContext(_ctx, getState());
		enterRule(_localctx, 64, RULE_equality);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(453);
			comparison();
			setState(458);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while (_la==T__43 || _la==T__44) {
				{
				{
				setState(454);
				_la = _input.LA(1);
				if ( !(_la==T__43 || _la==T__44) ) {
				_errHandler.recoverInline(this);
				}
				else {
					if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
					_errHandler.reportMatch(this);
					consume();
				}
				setState(455);
				comparison();
				}
				}
				setState(460);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ComparisonContext extends ParserRuleContext {
		public List<AdditiveContext> additive() {
			return getRuleContexts(AdditiveContext.class);
		}
		public AdditiveContext additive(int i) {
			return getRuleContext(AdditiveContext.class,i);
		}
		public ComparisonContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_comparison; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterComparison(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitComparison(this);
		}
	}

	public final ComparisonContext comparison() throws RecognitionException {
		ComparisonContext _localctx = new ComparisonContext(_ctx, getState());
		enterRule(_localctx, 66, RULE_comparison);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(461);
			additive();
			setState(466);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while ((((_la) & ~0x3f) == 0 && ((1L << _la) & 211106333196288L) != 0)) {
				{
				{
				setState(462);
				_la = _input.LA(1);
				if ( !((((_la) & ~0x3f) == 0 && ((1L << _la) & 211106333196288L) != 0)) ) {
				_errHandler.recoverInline(this);
				}
				else {
					if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
					_errHandler.reportMatch(this);
					consume();
				}
				setState(463);
				additive();
				}
				}
				setState(468);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class AdditiveContext extends ParserRuleContext {
		public List<MultiplicativeContext> multiplicative() {
			return getRuleContexts(MultiplicativeContext.class);
		}
		public MultiplicativeContext multiplicative(int i) {
			return getRuleContext(MultiplicativeContext.class,i);
		}
		public AdditiveContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_additive; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterAdditive(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitAdditive(this);
		}
	}

	public final AdditiveContext additive() throws RecognitionException {
		AdditiveContext _localctx = new AdditiveContext(_ctx, getState());
		enterRule(_localctx, 68, RULE_additive);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(469);
			multiplicative();
			setState(474);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while (_la==T__47 || _la==T__48) {
				{
				{
				setState(470);
				_la = _input.LA(1);
				if ( !(_la==T__47 || _la==T__48) ) {
				_errHandler.recoverInline(this);
				}
				else {
					if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
					_errHandler.reportMatch(this);
					consume();
				}
				setState(471);
				multiplicative();
				}
				}
				setState(476);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class MultiplicativeContext extends ParserRuleContext {
		public List<UnaryContext> unary() {
			return getRuleContexts(UnaryContext.class);
		}
		public UnaryContext unary(int i) {
			return getRuleContext(UnaryContext.class,i);
		}
		public MultiplicativeContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_multiplicative; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterMultiplicative(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitMultiplicative(this);
		}
	}

	public final MultiplicativeContext multiplicative() throws RecognitionException {
		MultiplicativeContext _localctx = new MultiplicativeContext(_ctx, getState());
		enterRule(_localctx, 70, RULE_multiplicative);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(477);
			unary();
			setState(482);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while ((((_la) & ~0x3f) == 0 && ((1L << _la) & 3377699728916480L) != 0)) {
				{
				{
				setState(478);
				_la = _input.LA(1);
				if ( !((((_la) & ~0x3f) == 0 && ((1L << _la) & 3377699728916480L) != 0)) ) {
				_errHandler.recoverInline(this);
				}
				else {
					if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
					_errHandler.reportMatch(this);
					consume();
				}
				setState(479);
				unary();
				}
				}
				setState(484);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class UnaryContext extends ParserRuleContext {
		public UnaryContext unary() {
			return getRuleContext(UnaryContext.class,0);
		}
		public PrimaryContext primary() {
			return getRuleContext(PrimaryContext.class,0);
		}
		public UnaryContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_unary; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterUnary(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitUnary(this);
		}
	}

	public final UnaryContext unary() throws RecognitionException {
		UnaryContext _localctx = new UnaryContext(_ctx, getState());
		enterRule(_localctx, 72, RULE_unary);
		int _la;
		try {
			setState(488);
			_errHandler.sync(this);
			switch (_input.LA(1)) {
			case T__47:
			case T__48:
			case T__51:
				enterOuterAlt(_localctx, 1);
				{
				setState(485);
				_la = _input.LA(1);
				if ( !((((_la) & ~0x3f) == 0 && ((1L << _la) & 5348024557502464L) != 0)) ) {
				_errHandler.recoverInline(this);
				}
				else {
					if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
					_errHandler.reportMatch(this);
					consume();
				}
				setState(486);
				unary();
				}
				break;
			case T__4:
			case T__6:
			case T__8:
			case T__13:
			case T__14:
			case T__15:
			case T__16:
			case T__18:
			case T__28:
			case T__30:
			case T__52:
			case T__53:
			case T__54:
			case T__55:
			case T__56:
			case T__57:
			case T__58:
			case T__59:
			case T__60:
			case T__61:
			case T__63:
			case T__64:
			case T__65:
			case T__66:
			case T__67:
			case T__68:
			case T__69:
			case T__70:
			case T__71:
			case T__72:
			case T__73:
			case T__74:
			case T__75:
			case T__76:
			case T__77:
			case T__78:
			case T__79:
			case T__80:
			case COLOR:
			case NUMBER:
			case STRING:
			case IDENT:
				enterOuterAlt(_localctx, 2);
				{
				setState(487);
				primary();
				}
				break;
			default:
				throw new NoViableAltException(this);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class PrimaryContext extends ParserRuleContext {
		public TerminalNode NUMBER() { return getToken(XrackParser.NUMBER, 0); }
		public TerminalNode UNIT() { return getToken(XrackParser.UNIT, 0); }
		public TerminalNode STRING() { return getToken(XrackParser.STRING, 0); }
		public TerminalNode COLOR() { return getToken(XrackParser.COLOR, 0); }
		public GradientContext gradient() {
			return getRuleContext(GradientContext.class,0);
		}
		public ImageUrlContext imageUrl() {
			return getRuleContext(ImageUrlContext.class,0);
		}
		public MarkdownContext markdown() {
			return getRuleContext(MarkdownContext.class,0);
		}
		public ArrayLiteralContext arrayLiteral() {
			return getRuleContext(ArrayLiteralContext.class,0);
		}
		public RecordLiteralContext recordLiteral() {
			return getRuleContext(RecordLiteralContext.class,0);
		}
		public CallContext call() {
			return getRuleContext(CallContext.class,0);
		}
		public ReferenceContext reference() {
			return getRuleContext(ReferenceContext.class,0);
		}
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public PrimaryContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_primary; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterPrimary(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitPrimary(this);
		}
	}

	public final PrimaryContext primary() throws RecognitionException {
		PrimaryContext _localctx = new PrimaryContext(_ctx, getState());
		enterRule(_localctx, 74, RULE_primary);
		int _la;
		try {
			setState(509);
			_errHandler.sync(this);
			switch ( getInterpreter().adaptivePredict(_input,55,_ctx) ) {
			case 1:
				enterOuterAlt(_localctx, 1);
				{
				setState(490);
				match(NUMBER);
				setState(492);
				_errHandler.sync(this);
				switch ( getInterpreter().adaptivePredict(_input,54,_ctx) ) {
				case 1:
					{
					setState(491);
					_la = _input.LA(1);
					if ( !(_la==T__50 || _la==UNIT) ) {
					_errHandler.recoverInline(this);
					}
					else {
						if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
						_errHandler.reportMatch(this);
						consume();
					}
					}
					break;
				}
				}
				break;
			case 2:
				enterOuterAlt(_localctx, 2);
				{
				setState(494);
				match(STRING);
				}
				break;
			case 3:
				enterOuterAlt(_localctx, 3);
				{
				setState(495);
				match(COLOR);
				}
				break;
			case 4:
				enterOuterAlt(_localctx, 4);
				{
				setState(496);
				match(T__52);
				}
				break;
			case 5:
				enterOuterAlt(_localctx, 5);
				{
				setState(497);
				match(T__53);
				}
				break;
			case 6:
				enterOuterAlt(_localctx, 6);
				{
				setState(498);
				gradient();
				}
				break;
			case 7:
				enterOuterAlt(_localctx, 7);
				{
				setState(499);
				imageUrl();
				}
				break;
			case 8:
				enterOuterAlt(_localctx, 8);
				{
				setState(500);
				markdown();
				}
				break;
			case 9:
				enterOuterAlt(_localctx, 9);
				{
				setState(501);
				arrayLiteral();
				}
				break;
			case 10:
				enterOuterAlt(_localctx, 10);
				{
				setState(502);
				recordLiteral();
				}
				break;
			case 11:
				enterOuterAlt(_localctx, 11);
				{
				setState(503);
				call();
				}
				break;
			case 12:
				enterOuterAlt(_localctx, 12);
				{
				setState(504);
				reference();
				}
				break;
			case 13:
				enterOuterAlt(_localctx, 13);
				{
				setState(505);
				match(T__30);
				setState(506);
				expression();
				setState(507);
				match(T__31);
				}
				break;
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class CallContext extends ParserRuleContext {
		public ReferenceContext reference() {
			return getRuleContext(ReferenceContext.class,0);
		}
		public List<ExpressionContext> expression() {
			return getRuleContexts(ExpressionContext.class);
		}
		public ExpressionContext expression(int i) {
			return getRuleContext(ExpressionContext.class,i);
		}
		public CallContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_call; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterCall(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitCall(this);
		}
	}

	public final CallContext call() throws RecognitionException {
		CallContext _localctx = new CallContext(_ctx, getState());
		enterRule(_localctx, 76, RULE_call);
		int _la;
		try {
			int _alt;
			enterOuterAlt(_localctx, 1);
			{
			setState(511);
			reference();
			setState(512);
			match(T__30);
			setState(524);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if ((((_la) & ~0x3f) == 0 && ((1L << _la) & 9219712864842662560L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 8126463L) != 0)) {
				{
				setState(513);
				expression();
				setState(518);
				_errHandler.sync(this);
				_alt = getInterpreter().adaptivePredict(_input,56,_ctx);
				while ( _alt!=2 && _alt!=org.antlr.v4.runtime.atn.ATN.INVALID_ALT_NUMBER ) {
					if ( _alt==1 ) {
						{
						{
						setState(514);
						match(T__2);
						setState(515);
						expression();
						}
						} 
					}
					setState(520);
					_errHandler.sync(this);
					_alt = getInterpreter().adaptivePredict(_input,56,_ctx);
				}
				setState(522);
				_errHandler.sync(this);
				_la = _input.LA(1);
				if (_la==T__2) {
					{
					setState(521);
					match(T__2);
					}
				}

				}
			}

			setState(526);
			match(T__31);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ArrayLiteralContext extends ParserRuleContext {
		public List<ExpressionContext> expression() {
			return getRuleContexts(ExpressionContext.class);
		}
		public ExpressionContext expression(int i) {
			return getRuleContext(ExpressionContext.class,i);
		}
		public ArrayLiteralContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_arrayLiteral; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterArrayLiteral(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitArrayLiteral(this);
		}
	}

	public final ArrayLiteralContext arrayLiteral() throws RecognitionException {
		ArrayLiteralContext _localctx = new ArrayLiteralContext(_ctx, getState());
		enterRule(_localctx, 78, RULE_arrayLiteral);
		int _la;
		try {
			int _alt;
			enterOuterAlt(_localctx, 1);
			{
			setState(528);
			match(T__16);
			setState(540);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if ((((_la) & ~0x3f) == 0 && ((1L << _la) & 9219712864842662560L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 8126463L) != 0)) {
				{
				setState(529);
				expression();
				setState(534);
				_errHandler.sync(this);
				_alt = getInterpreter().adaptivePredict(_input,59,_ctx);
				while ( _alt!=2 && _alt!=org.antlr.v4.runtime.atn.ATN.INVALID_ALT_NUMBER ) {
					if ( _alt==1 ) {
						{
						{
						setState(530);
						match(T__2);
						setState(531);
						expression();
						}
						} 
					}
					setState(536);
					_errHandler.sync(this);
					_alt = getInterpreter().adaptivePredict(_input,59,_ctx);
				}
				setState(538);
				_errHandler.sync(this);
				_la = _input.LA(1);
				if (_la==T__2) {
					{
					setState(537);
					match(T__2);
					}
				}

				}
			}

			setState(542);
			match(T__17);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class RecordLiteralContext extends ParserRuleContext {
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public List<RecordValueContext> recordValue() {
			return getRuleContexts(RecordValueContext.class);
		}
		public RecordValueContext recordValue(int i) {
			return getRuleContext(RecordValueContext.class,i);
		}
		public RecordLiteralContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_recordLiteral; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterRecordLiteral(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitRecordLiteral(this);
		}
	}

	public final RecordLiteralContext recordLiteral() throws RecognitionException {
		RecordLiteralContext _localctx = new RecordLiteralContext(_ctx, getState());
		enterRule(_localctx, 80, RULE_recordLiteral);
		int _la;
		try {
			int _alt;
			enterOuterAlt(_localctx, 1);
			{
			setState(544);
			identifier();
			setState(545);
			match(T__1);
			setState(557);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if ((((_la) & ~0x3f) == 0 && ((1L << _la) & 1729382257447781024L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 4456447L) != 0)) {
				{
				setState(546);
				recordValue();
				setState(551);
				_errHandler.sync(this);
				_alt = getInterpreter().adaptivePredict(_input,62,_ctx);
				while ( _alt!=2 && _alt!=org.antlr.v4.runtime.atn.ATN.INVALID_ALT_NUMBER ) {
					if ( _alt==1 ) {
						{
						{
						setState(547);
						match(T__2);
						setState(548);
						recordValue();
						}
						} 
					}
					setState(553);
					_errHandler.sync(this);
					_alt = getInterpreter().adaptivePredict(_input,62,_ctx);
				}
				setState(555);
				_errHandler.sync(this);
				_la = _input.LA(1);
				if (_la==T__2) {
					{
					setState(554);
					match(T__2);
					}
				}

				}
			}

			setState(559);
			match(T__3);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class RecordValueContext extends ParserRuleContext {
		public IdentifierContext identifier() {
			return getRuleContext(IdentifierContext.class,0);
		}
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public RecordValueContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_recordValue; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterRecordValue(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitRecordValue(this);
		}
	}

	public final RecordValueContext recordValue() throws RecognitionException {
		RecordValueContext _localctx = new RecordValueContext(_ctx, getState());
		enterRule(_localctx, 82, RULE_recordValue);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(561);
			identifier();
			setState(562);
			match(T__9);
			setState(563);
			expression();
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ImageUrlContext extends ParserRuleContext {
		public TerminalNode STRING() { return getToken(XrackParser.STRING, 0); }
		public ImageSliceContext imageSlice() {
			return getRuleContext(ImageSliceContext.class,0);
		}
		public ImageUrlContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_imageUrl; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterImageUrl(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitImageUrl(this);
		}
	}

	public final ImageUrlContext imageUrl() throws RecognitionException {
		ImageUrlContext _localctx = new ImageUrlContext(_ctx, getState());
		enterRule(_localctx, 84, RULE_imageUrl);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(565);
			match(T__54);
			setState(566);
			match(T__30);
			setState(567);
			match(STRING);
			setState(570);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__2) {
				{
				setState(568);
				match(T__2);
				setState(569);
				imageSlice();
				}
			}

			setState(572);
			match(T__31);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ImageSliceContext extends ParserRuleContext {
		public TerminalNode IDENT() { return getToken(XrackParser.IDENT, 0); }
		public List<ImageEdgeContext> imageEdge() {
			return getRuleContexts(ImageEdgeContext.class);
		}
		public ImageEdgeContext imageEdge(int i) {
			return getRuleContext(ImageEdgeContext.class,i);
		}
		public ImageSliceContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_imageSlice; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterImageSlice(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitImageSlice(this);
		}
	}

	public final ImageSliceContext imageSlice() throws RecognitionException {
		ImageSliceContext _localctx = new ImageSliceContext(_ctx, getState());
		enterRule(_localctx, 86, RULE_imageSlice);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(574);
			match(IDENT);
			setState(575);
			match(T__30);
			setState(577); 
			_errHandler.sync(this);
			_la = _input.LA(1);
			do {
				{
				{
				setState(576);
				imageEdge();
				}
				}
				setState(579); 
				_errHandler.sync(this);
				_la = _input.LA(1);
			} while ( _la==NUMBER );
			setState(581);
			match(T__31);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ImageEdgeContext extends ParserRuleContext {
		public TerminalNode NUMBER() { return getToken(XrackParser.NUMBER, 0); }
		public ImageEdgeContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_imageEdge; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterImageEdge(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitImageEdge(this);
		}
	}

	public final ImageEdgeContext imageEdge() throws RecognitionException {
		ImageEdgeContext _localctx = new ImageEdgeContext(_ctx, getState());
		enterRule(_localctx, 88, RULE_imageEdge);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(583);
			match(NUMBER);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class MarkdownContext extends ParserRuleContext {
		public TerminalNode STRING() { return getToken(XrackParser.STRING, 0); }
		public MarkdownContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_markdown; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterMarkdown(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitMarkdown(this);
		}
	}

	public final MarkdownContext markdown() throws RecognitionException {
		MarkdownContext _localctx = new MarkdownContext(_ctx, getState());
		enterRule(_localctx, 90, RULE_markdown);
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(585);
			match(T__55);
			setState(586);
			match(T__30);
			setState(587);
			match(STRING);
			setState(588);
			match(T__31);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class GradientContext extends ParserRuleContext {
		public List<GradientStopContext> gradientStop() {
			return getRuleContexts(GradientStopContext.class);
		}
		public GradientStopContext gradientStop(int i) {
			return getRuleContext(GradientStopContext.class,i);
		}
		public GradientAngleContext gradientAngle() {
			return getRuleContext(GradientAngleContext.class,0);
		}
		public GradientContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_gradient; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterGradient(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitGradient(this);
		}
	}

	public final GradientContext gradient() throws RecognitionException {
		GradientContext _localctx = new GradientContext(_ctx, getState());
		enterRule(_localctx, 92, RULE_gradient);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(596);
			_errHandler.sync(this);
			switch (_input.LA(1)) {
			case T__56:
				{
				setState(590);
				match(T__56);
				setState(591);
				match(T__30);
				setState(592);
				gradientAngle();
				}
				break;
			case T__57:
				{
				setState(593);
				match(T__57);
				setState(594);
				match(T__30);
				setState(595);
				match(T__58);
				}
				break;
			default:
				throw new NoViableAltException(this);
			}
			setState(598);
			match(T__2);
			setState(599);
			gradientStop();
			setState(604);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while (_la==T__2) {
				{
				{
				setState(600);
				match(T__2);
				setState(601);
				gradientStop();
				}
				}
				setState(606);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			setState(607);
			match(T__31);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class GradientAngleContext extends ParserRuleContext {
		public TerminalNode NUMBER() { return getToken(XrackParser.NUMBER, 0); }
		public GradientAngleContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_gradientAngle; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterGradientAngle(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitGradientAngle(this);
		}
	}

	public final GradientAngleContext gradientAngle() throws RecognitionException {
		GradientAngleContext _localctx = new GradientAngleContext(_ctx, getState());
		enterRule(_localctx, 94, RULE_gradientAngle);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(610);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==T__47 || _la==T__48) {
				{
				setState(609);
				_la = _input.LA(1);
				if ( !(_la==T__47 || _la==T__48) ) {
				_errHandler.recoverInline(this);
				}
				else {
					if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
					_errHandler.reportMatch(this);
					consume();
				}
				}
			}

			setState(612);
			match(NUMBER);
			setState(613);
			match(T__59);
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class GradientStopContext extends ParserRuleContext {
		public ExpressionContext expression() {
			return getRuleContext(ExpressionContext.class,0);
		}
		public TerminalNode NUMBER() { return getToken(XrackParser.NUMBER, 0); }
		public GradientStopContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_gradientStop; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterGradientStop(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitGradientStop(this);
		}
	}

	public final GradientStopContext gradientStop() throws RecognitionException {
		GradientStopContext _localctx = new GradientStopContext(_ctx, getState());
		enterRule(_localctx, 96, RULE_gradientStop);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(615);
			expression();
			setState(618);
			_errHandler.sync(this);
			_la = _input.LA(1);
			if (_la==NUMBER) {
				{
				setState(616);
				match(NUMBER);
				setState(617);
				match(T__50);
				}
			}

			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class ReferenceContext extends ParserRuleContext {
		public List<IdentifierContext> identifier() {
			return getRuleContexts(IdentifierContext.class);
		}
		public IdentifierContext identifier(int i) {
			return getRuleContext(IdentifierContext.class,i);
		}
		public ReferenceContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_reference; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterReference(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitReference(this);
		}
	}

	public final ReferenceContext reference() throws RecognitionException {
		ReferenceContext _localctx = new ReferenceContext(_ctx, getState());
		enterRule(_localctx, 98, RULE_reference);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(623);
			_errHandler.sync(this);
			switch (_input.LA(1)) {
			case T__60:
				{
				setState(620);
				match(T__60);
				}
				break;
			case T__61:
				{
				setState(621);
				match(T__61);
				}
				break;
			case T__4:
			case T__6:
			case T__8:
			case T__13:
			case T__14:
			case T__15:
			case T__18:
			case T__28:
			case T__58:
			case T__59:
			case T__63:
			case T__64:
			case T__65:
			case T__66:
			case T__67:
			case T__68:
			case T__69:
			case T__70:
			case T__71:
			case T__72:
			case T__73:
			case T__74:
			case T__75:
			case T__76:
			case T__77:
			case T__78:
			case T__79:
			case T__80:
			case IDENT:
				{
				setState(622);
				identifier();
				}
				break;
			default:
				throw new NoViableAltException(this);
			}
			setState(629);
			_errHandler.sync(this);
			_la = _input.LA(1);
			while (_la==T__62) {
				{
				{
				setState(625);
				match(T__62);
				setState(626);
				identifier();
				}
				}
				setState(631);
				_errHandler.sync(this);
				_la = _input.LA(1);
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	@SuppressWarnings("CheckReturnValue")
	public static class IdentifierContext extends ParserRuleContext {
		public TerminalNode IDENT() { return getToken(XrackParser.IDENT, 0); }
		public IdentifierContext(ParserRuleContext parent, int invokingState) {
			super(parent, invokingState);
		}
		@Override public int getRuleIndex() { return RULE_identifier; }
		@Override
		public void enterRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).enterIdentifier(this);
		}
		@Override
		public void exitRule(ParseTreeListener listener) {
			if ( listener instanceof XrackListener ) ((XrackListener)listener).exitIdentifier(this);
		}
	}

	public final IdentifierContext identifier() throws RecognitionException {
		IdentifierContext _localctx = new IdentifierContext(_ctx, getState());
		enterRule(_localctx, 100, RULE_identifier);
		int _la;
		try {
			enterOuterAlt(_localctx, 1);
			{
			setState(632);
			_la = _input.LA(1);
			if ( !((((_la) & ~0x3f) == 0 && ((1L << _la) & 1729382257447781024L) != 0) || ((((_la - 64)) & ~0x3f) == 0 && ((1L << (_la - 64)) & 4456447L) != 0)) ) {
			_errHandler.recoverInline(this);
			}
			else {
				if ( _input.LA(1)==Token.EOF ) matchedEOF = true;
				_errHandler.reportMatch(this);
				consume();
			}
			}
		}
		catch (RecognitionException re) {
			_localctx.exception = re;
			_errHandler.reportError(this, re);
			_errHandler.recover(this, re);
		}
		finally {
			exitRule();
		}
		return _localctx;
	}

	public static final String _serializedATN =
		"\u0004\u0001Y\u027b\u0002\u0000\u0007\u0000\u0002\u0001\u0007\u0001\u0002"+
		"\u0002\u0007\u0002\u0002\u0003\u0007\u0003\u0002\u0004\u0007\u0004\u0002"+
		"\u0005\u0007\u0005\u0002\u0006\u0007\u0006\u0002\u0007\u0007\u0007\u0002"+
		"\b\u0007\b\u0002\t\u0007\t\u0002\n\u0007\n\u0002\u000b\u0007\u000b\u0002"+
		"\f\u0007\f\u0002\r\u0007\r\u0002\u000e\u0007\u000e\u0002\u000f\u0007\u000f"+
		"\u0002\u0010\u0007\u0010\u0002\u0011\u0007\u0011\u0002\u0012\u0007\u0012"+
		"\u0002\u0013\u0007\u0013\u0002\u0014\u0007\u0014\u0002\u0015\u0007\u0015"+
		"\u0002\u0016\u0007\u0016\u0002\u0017\u0007\u0017\u0002\u0018\u0007\u0018"+
		"\u0002\u0019\u0007\u0019\u0002\u001a\u0007\u001a\u0002\u001b\u0007\u001b"+
		"\u0002\u001c\u0007\u001c\u0002\u001d\u0007\u001d\u0002\u001e\u0007\u001e"+
		"\u0002\u001f\u0007\u001f\u0002 \u0007 \u0002!\u0007!\u0002\"\u0007\"\u0002"+
		"#\u0007#\u0002$\u0007$\u0002%\u0007%\u0002&\u0007&\u0002\'\u0007\'\u0002"+
		"(\u0007(\u0002)\u0007)\u0002*\u0007*\u0002+\u0007+\u0002,\u0007,\u0002"+
		"-\u0007-\u0002.\u0007.\u0002/\u0007/\u00020\u00070\u00021\u00071\u0002"+
		"2\u00072\u0001\u0000\u0001\u0000\u0005\u0000i\b\u0000\n\u0000\f\u0000"+
		"l\t\u0000\u0001\u0000\u0001\u0000\u0004\u0000p\b\u0000\u000b\u0000\f\u0000"+
		"q\u0001\u0000\u0001\u0000\u0001\u0001\u0001\u0001\u0001\u0001\u0001\u0001"+
		"\u0001\u0001\u0005\u0001{\b\u0001\n\u0001\f\u0001~\t\u0001\u0001\u0001"+
		"\u0003\u0001\u0081\b\u0001\u0001\u0001\u0001\u0001\u0001\u0001\u0001\u0001"+
		"\u0001\u0001\u0001\u0002\u0001\u0002\u0001\u0002\u0001\u0002\u0001\u0003"+
		"\u0001\u0003\u0001\u0003\u0003\u0003\u008f\b\u0003\u0001\u0004\u0003\u0004"+
		"\u0092\b\u0004\u0001\u0004\u0001\u0004\u0001\u0004\u0001\u0004\u0001\u0004"+
		"\u0001\u0004\u0005\u0004\u009a\b\u0004\n\u0004\f\u0004\u009d\t\u0004\u0001"+
		"\u0004\u0003\u0004\u00a0\b\u0004\u0001\u0004\u0001\u0004\u0001\u0005\u0001"+
		"\u0005\u0001\u0005\u0001\u0005\u0001\u0006\u0003\u0006\u00a9\b\u0006\u0001"+
		"\u0006\u0001\u0006\u0001\u0006\u0001\u0006\u0001\u0006\u0001\u0006\u0005"+
		"\u0006\u00b1\b\u0006\n\u0006\f\u0006\u00b4\t\u0006\u0001\u0006\u0001\u0006"+
		"\u0001\u0007\u0001\u0007\u0001\u0007\u0001\u0007\u0001\u0007\u0001\u0007"+
		"\u0001\u0007\u0003\u0007\u00bf\b\u0007\u0001\b\u0001\b\u0001\b\u0001\b"+
		"\u0005\b\u00c5\b\b\n\b\f\b\u00c8\t\b\u0001\b\u0001\b\u0001\t\u0001\t\u0005"+
		"\t\u00ce\b\t\n\t\f\t\u00d1\t\t\u0001\t\u0001\t\u0001\n\u0001\n\u0003\n"+
		"\u00d7\b\n\u0001\n\u0001\n\u0001\n\u0001\n\u0003\n\u00dd\b\n\u0001\n\u0001"+
		"\n\u0001\u000b\u0001\u000b\u0001\u000b\u0005\u000b\u00e4\b\u000b\n\u000b"+
		"\f\u000b\u00e7\t\u000b\u0001\u000b\u0001\u000b\u0001\f\u0001\f\u0001\f"+
		"\u0003\f\u00ee\b\f\u0001\f\u0001\f\u0001\f\u0001\f\u0005\f\u00f4\b\f\n"+
		"\f\f\f\u00f7\t\f\u0001\f\u0001\f\u0001\r\u0001\r\u0001\r\u0001\r\u0001"+
		"\r\u0001\u000e\u0001\u000e\u0001\u000e\u0005\u000e\u0103\b\u000e\n\u000e"+
		"\f\u000e\u0106\t\u000e\u0001\u000e\u0001\u000e\u0001\u000f\u0001\u000f"+
		"\u0001\u000f\u0001\u000f\u0005\u000f\u010e\b\u000f\n\u000f\f\u000f\u0111"+
		"\t\u000f\u0001\u000f\u0003\u000f\u0114\b\u000f\u0001\u000f\u0001\u000f"+
		"\u0001\u0010\u0003\u0010\u0119\b\u0010\u0001\u0010\u0001\u0010\u0001\u0010"+
		"\u0001\u0010\u0001\u0010\u0001\u0010\u0001\u0010\u0001\u0010\u0001\u0010"+
		"\u0003\u0010\u0124\b\u0010\u0001\u0010\u0001\u0010\u0001\u0011\u0001\u0011"+
		"\u0001\u0012\u0001\u0012\u0001\u0012\u0001\u0012\u0001\u0012\u0003\u0012"+
		"\u012f\b\u0012\u0001\u0013\u0003\u0013\u0132\b\u0013\u0001\u0013\u0001"+
		"\u0013\u0001\u0013\u0001\u0013\u0001\u0013\u0001\u0013\u0001\u0013\u0001"+
		"\u0013\u0005\u0013\u013c\b\u0013\n\u0013\f\u0013\u013f\t\u0013\u0001\u0013"+
		"\u0003\u0013\u0142\b\u0013\u0003\u0013\u0144\b\u0013\u0001\u0013\u0001"+
		"\u0013\u0001\u0013\u0003\u0013\u0149\b\u0013\u0003\u0013\u014b\b\u0013"+
		"\u0001\u0013\u0001\u0013\u0001\u0014\u0001\u0014\u0001\u0014\u0003\u0014"+
		"\u0152\b\u0014\u0001\u0014\u0001\u0014\u0001\u0015\u0001\u0015\u0001\u0015"+
		"\u0001\u0015\u0001\u0015\u0003\u0015\u015b\b\u0015\u0001\u0015\u0001\u0015"+
		"\u0001\u0016\u0001\u0016\u0001\u0016\u0001\u0016\u0001\u0016\u0005\u0016"+
		"\u0164\b\u0016\n\u0016\f\u0016\u0167\t\u0016\u0003\u0016\u0169\b\u0016"+
		"\u0001\u0016\u0003\u0016\u016c\b\u0016\u0001\u0016\u0001\u0016\u0001\u0016"+
		"\u0005\u0016\u0171\b\u0016\n\u0016\f\u0016\u0174\t\u0016\u0001\u0016\u0001"+
		"\u0016\u0003\u0016\u0178\b\u0016\u0003\u0016\u017a\b\u0016\u0001\u0016"+
		"\u0001\u0016\u0001\u0017\u0001\u0017\u0001\u0017\u0001\u0017\u0001\u0017"+
		"\u0003\u0017\u0183\b\u0017\u0001\u0017\u0001\u0017\u0001\u0018\u0001\u0018"+
		"\u0001\u0019\u0001\u0019\u0003\u0019\u018b\b\u0019\u0001\u0019\u0001\u0019"+
		"\u0001\u0019\u0003\u0019\u0190\b\u0019\u0001\u0019\u0001\u0019\u0001\u0019"+
		"\u0005\u0019\u0195\b\u0019\n\u0019\f\u0019\u0198\t\u0019\u0001\u0019\u0001"+
		"\u0019\u0001\u001a\u0001\u001a\u0001\u001a\u0001\u001a\u0001\u001b\u0001"+
		"\u001b\u0001\u001b\u0001\u001b\u0001\u001b\u0001\u001b\u0003\u001b\u01a6"+
		"\b\u001b\u0001\u001b\u0001\u001b\u0001\u001b\u0001\u001b\u0001\u001c\u0001"+
		"\u001c\u0001\u001d\u0001\u001d\u0001\u001d\u0001\u001d\u0001\u001d\u0001"+
		"\u001d\u0003\u001d\u01b4\b\u001d\u0001\u001e\u0001\u001e\u0001\u001e\u0005"+
		"\u001e\u01b9\b\u001e\n\u001e\f\u001e\u01bc\t\u001e\u0001\u001f\u0001\u001f"+
		"\u0001\u001f\u0005\u001f\u01c1\b\u001f\n\u001f\f\u001f\u01c4\t\u001f\u0001"+
		" \u0001 \u0001 \u0005 \u01c9\b \n \f \u01cc\t \u0001!\u0001!\u0001!\u0005"+
		"!\u01d1\b!\n!\f!\u01d4\t!\u0001\"\u0001\"\u0001\"\u0005\"\u01d9\b\"\n"+
		"\"\f\"\u01dc\t\"\u0001#\u0001#\u0001#\u0005#\u01e1\b#\n#\f#\u01e4\t#\u0001"+
		"$\u0001$\u0001$\u0003$\u01e9\b$\u0001%\u0001%\u0003%\u01ed\b%\u0001%\u0001"+
		"%\u0001%\u0001%\u0001%\u0001%\u0001%\u0001%\u0001%\u0001%\u0001%\u0001"+
		"%\u0001%\u0001%\u0001%\u0003%\u01fe\b%\u0001&\u0001&\u0001&\u0001&\u0001"+
		"&\u0005&\u0205\b&\n&\f&\u0208\t&\u0001&\u0003&\u020b\b&\u0003&\u020d\b"+
		"&\u0001&\u0001&\u0001\'\u0001\'\u0001\'\u0001\'\u0005\'\u0215\b\'\n\'"+
		"\f\'\u0218\t\'\u0001\'\u0003\'\u021b\b\'\u0003\'\u021d\b\'\u0001\'\u0001"+
		"\'\u0001(\u0001(\u0001(\u0001(\u0001(\u0005(\u0226\b(\n(\f(\u0229\t(\u0001"+
		"(\u0003(\u022c\b(\u0003(\u022e\b(\u0001(\u0001(\u0001)\u0001)\u0001)\u0001"+
		")\u0001*\u0001*\u0001*\u0001*\u0001*\u0003*\u023b\b*\u0001*\u0001*\u0001"+
		"+\u0001+\u0001+\u0004+\u0242\b+\u000b+\f+\u0243\u0001+\u0001+\u0001,\u0001"+
		",\u0001-\u0001-\u0001-\u0001-\u0001-\u0001.\u0001.\u0001.\u0001.\u0001"+
		".\u0001.\u0003.\u0255\b.\u0001.\u0001.\u0001.\u0001.\u0005.\u025b\b.\n"+
		".\f.\u025e\t.\u0001.\u0001.\u0001/\u0003/\u0263\b/\u0001/\u0001/\u0001"+
		"/\u00010\u00010\u00010\u00030\u026b\b0\u00011\u00011\u00011\u00031\u0270"+
		"\b1\u00011\u00011\u00051\u0274\b1\n1\f1\u0277\t1\u00012\u00012\u00012"+
		"\u0000\u00003\u0000\u0002\u0004\u0006\b\n\f\u000e\u0010\u0012\u0014\u0016"+
		"\u0018\u001a\u001c\u001e \"$&(*,.02468:<>@BDFHJLNPRTVXZ\\^`bd\u0000\n"+
		"\u0001\u0000\u0014\u0016\u0002\u0000\u0014\u0016\u001c\u001c\u0001\u0000"+
		"#%\u0001\u0000,-\u0002\u0000\u0019\u001a./\u0001\u000001\u0002\u0000\u0017"+
		"\u001723\u0002\u00000144\u0002\u000033RR\t\u0000\u0005\u0005\u0007\u0007"+
		"\t\t\u000e\u0010\u0013\u0013\u001d\u001d;<@QVV\u02a4\u0000j\u0001\u0000"+
		"\u0000\u0000\u0002u\u0001\u0000\u0000\u0000\u0004\u0087\u0001\u0000\u0000"+
		"\u0000\u0006\u008b\u0001\u0000\u0000\u0000\b\u0091\u0001\u0000\u0000\u0000"+
		"\n\u00a3\u0001\u0000\u0000\u0000\f\u00a8\u0001\u0000\u0000\u0000\u000e"+
		"\u00be\u0001\u0000\u0000\u0000\u0010\u00c0\u0001\u0000\u0000\u0000\u0012"+
		"\u00cb\u0001\u0000\u0000\u0000\u0014\u00d6\u0001\u0000\u0000\u0000\u0016"+
		"\u00e0\u0001\u0000\u0000\u0000\u0018\u00ea\u0001\u0000\u0000\u0000\u001a"+
		"\u00fa\u0001\u0000\u0000\u0000\u001c\u00ff\u0001\u0000\u0000\u0000\u001e"+
		"\u0109\u0001\u0000\u0000\u0000 \u0118\u0001\u0000\u0000\u0000\"\u0127"+
		"\u0001\u0000\u0000\u0000$\u012e\u0001\u0000\u0000\u0000&\u0131\u0001\u0000"+
		"\u0000\u0000(\u0151\u0001\u0000\u0000\u0000*\u0155\u0001\u0000\u0000\u0000"+
		",\u015e\u0001\u0000\u0000\u0000.\u0182\u0001\u0000\u0000\u00000\u0186"+
		"\u0001\u0000\u0000\u00002\u018a\u0001\u0000\u0000\u00004\u019b\u0001\u0000"+
		"\u0000\u00006\u019f\u0001\u0000\u0000\u00008\u01ab\u0001\u0000\u0000\u0000"+
		":\u01ad\u0001\u0000\u0000\u0000<\u01b5\u0001\u0000\u0000\u0000>\u01bd"+
		"\u0001\u0000\u0000\u0000@\u01c5\u0001\u0000\u0000\u0000B\u01cd\u0001\u0000"+
		"\u0000\u0000D\u01d5\u0001\u0000\u0000\u0000F\u01dd\u0001\u0000\u0000\u0000"+
		"H\u01e8\u0001\u0000\u0000\u0000J\u01fd\u0001\u0000\u0000\u0000L\u01ff"+
		"\u0001\u0000\u0000\u0000N\u0210\u0001\u0000\u0000\u0000P\u0220\u0001\u0000"+
		"\u0000\u0000R\u0231\u0001\u0000\u0000\u0000T\u0235\u0001\u0000\u0000\u0000"+
		"V\u023e\u0001\u0000\u0000\u0000X\u0247\u0001\u0000\u0000\u0000Z\u0249"+
		"\u0001\u0000\u0000\u0000\\\u0254\u0001\u0000\u0000\u0000^\u0262\u0001"+
		"\u0000\u0000\u0000`\u0267\u0001\u0000\u0000\u0000b\u026f\u0001\u0000\u0000"+
		"\u0000d\u0278\u0001\u0000\u0000\u0000fi\u0003\u0002\u0001\u0000gi\u0003"+
		"\u0004\u0002\u0000hf\u0001\u0000\u0000\u0000hg\u0001\u0000\u0000\u0000"+
		"il\u0001\u0000\u0000\u0000jh\u0001\u0000\u0000\u0000jk\u0001\u0000\u0000"+
		"\u0000ko\u0001\u0000\u0000\u0000lj\u0001\u0000\u0000\u0000mp\u0003\b\u0004"+
		"\u0000np\u0003\f\u0006\u0000om\u0001\u0000\u0000\u0000on\u0001\u0000\u0000"+
		"\u0000pq\u0001\u0000\u0000\u0000qo\u0001\u0000\u0000\u0000qr\u0001\u0000"+
		"\u0000\u0000rs\u0001\u0000\u0000\u0000st\u0005\u0000\u0000\u0001t\u0001"+
		"\u0001\u0000\u0000\u0000uv\u0005\u0001\u0000\u0000vw\u0005\u0002\u0000"+
		"\u0000w|\u0003\u0006\u0003\u0000xy\u0005\u0003\u0000\u0000y{\u0003\u0006"+
		"\u0003\u0000zx\u0001\u0000\u0000\u0000{~\u0001\u0000\u0000\u0000|z\u0001"+
		"\u0000\u0000\u0000|}\u0001\u0000\u0000\u0000}\u0080\u0001\u0000\u0000"+
		"\u0000~|\u0001\u0000\u0000\u0000\u007f\u0081\u0005\u0003\u0000\u0000\u0080"+
		"\u007f\u0001\u0000\u0000\u0000\u0080\u0081\u0001\u0000\u0000\u0000\u0081"+
		"\u0082\u0001\u0000\u0000\u0000\u0082\u0083\u0005\u0004\u0000\u0000\u0083"+
		"\u0084\u0005\u0005\u0000\u0000\u0084\u0085\u0005U\u0000\u0000\u0085\u0086"+
		"\u0005\u0006\u0000\u0000\u0086\u0003\u0001\u0000\u0000\u0000\u0087\u0088"+
		"\u0005\u0001\u0000\u0000\u0088\u0089\u0005U\u0000\u0000\u0089\u008a\u0005"+
		"\u0006\u0000\u0000\u008a\u0005\u0001\u0000\u0000\u0000\u008b\u008e\u0003"+
		"d2\u0000\u008c\u008d\u0005\u0007\u0000\u0000\u008d\u008f\u0003d2\u0000"+
		"\u008e\u008c\u0001\u0000\u0000\u0000\u008e\u008f\u0001\u0000\u0000\u0000"+
		"\u008f\u0007\u0001\u0000\u0000\u0000\u0090\u0092\u0005\b\u0000\u0000\u0091"+
		"\u0090\u0001\u0000\u0000\u0000\u0091\u0092\u0001\u0000\u0000\u0000\u0092"+
		"\u0093\u0001\u0000\u0000\u0000\u0093\u0094\u0005\t\u0000\u0000\u0094\u0095"+
		"\u0003d2\u0000\u0095\u0096\u0005\u0002\u0000\u0000\u0096\u009b\u0003\n"+
		"\u0005\u0000\u0097\u0098\u0005\u0003\u0000\u0000\u0098\u009a\u0003\n\u0005"+
		"\u0000\u0099\u0097\u0001\u0000\u0000\u0000\u009a\u009d\u0001\u0000\u0000"+
		"\u0000\u009b\u0099\u0001\u0000\u0000\u0000\u009b\u009c\u0001\u0000\u0000"+
		"\u0000\u009c\u009f\u0001\u0000\u0000\u0000\u009d\u009b\u0001\u0000\u0000"+
		"\u0000\u009e\u00a0\u0005\u0003\u0000\u0000\u009f\u009e\u0001\u0000\u0000"+
		"\u0000\u009f\u00a0\u0001\u0000\u0000\u0000\u00a0\u00a1\u0001\u0000\u0000"+
		"\u0000\u00a1\u00a2\u0005\u0004\u0000\u0000\u00a2\t\u0001\u0000\u0000\u0000"+
		"\u00a3\u00a4\u0003d2\u0000\u00a4\u00a5\u0005\n\u0000\u0000\u00a5\u00a6"+
		"\u0003$\u0012\u0000\u00a6\u000b\u0001\u0000\u0000\u0000\u00a7\u00a9\u0005"+
		"\b\u0000\u0000\u00a8\u00a7\u0001\u0000\u0000\u0000\u00a8\u00a9\u0001\u0000"+
		"\u0000\u0000\u00a9\u00aa\u0001\u0000\u0000\u0000\u00aa\u00ab\u0005\u000b"+
		"\u0000\u0000\u00ab\u00ac\u0003d2\u0000\u00ac\u00ad\u0005\f\u0000\u0000"+
		"\u00ad\u00ae\u0003d2\u0000\u00ae\u00b2\u0005\u0002\u0000\u0000\u00af\u00b1"+
		"\u0003\u000e\u0007\u0000\u00b0\u00af\u0001\u0000\u0000\u0000\u00b1\u00b4"+
		"\u0001\u0000\u0000\u0000\u00b2\u00b0\u0001\u0000\u0000\u0000\u00b2\u00b3"+
		"\u0001\u0000\u0000\u0000\u00b3\u00b5\u0001\u0000\u0000\u0000\u00b4\u00b2"+
		"\u0001\u0000\u0000\u0000\u00b5\u00b6\u0005\u0004\u0000\u0000\u00b6\r\u0001"+
		"\u0000\u0000\u0000\u00b7\u00bf\u0003 \u0010\u0000\u00b8\u00bf\u0003&\u0013"+
		"\u0000\u00b9\u00bf\u0003*\u0015\u0000\u00ba\u00bf\u0003,\u0016\u0000\u00bb"+
		"\u00bf\u00032\u0019\u0000\u00bc\u00bf\u0003\u0010\b\u0000\u00bd\u00bf"+
		"\u0003\u0016\u000b\u0000\u00be\u00b7\u0001\u0000\u0000\u0000\u00be\u00b8"+
		"\u0001\u0000\u0000\u0000\u00be\u00b9\u0001\u0000\u0000\u0000\u00be\u00ba"+
		"\u0001\u0000\u0000\u0000\u00be\u00bb\u0001\u0000\u0000\u0000\u00be\u00bc"+
		"\u0001\u0000\u0000\u0000\u00be\u00bd\u0001\u0000\u0000\u0000\u00bf\u000f"+
		"\u0001\u0000\u0000\u0000\u00c0\u00c1\u0005\r\u0000\u0000\u00c1\u00c6\u0003"+
		"d2\u0000\u00c2\u00c3\u0005\u0003\u0000\u0000\u00c3\u00c5\u0003d2\u0000"+
		"\u00c4\u00c2\u0001\u0000\u0000\u0000\u00c5\u00c8\u0001\u0000\u0000\u0000"+
		"\u00c6\u00c4\u0001\u0000\u0000\u0000\u00c6\u00c7\u0001\u0000\u0000\u0000"+
		"\u00c7\u00c9\u0001\u0000\u0000\u0000\u00c8\u00c6\u0001\u0000\u0000\u0000"+
		"\u00c9\u00ca\u0003\u0012\t\u0000\u00ca\u0011\u0001\u0000\u0000\u0000\u00cb"+
		"\u00cf\u0005\u0002\u0000\u0000\u00cc\u00ce\u0003\u0014\n\u0000\u00cd\u00cc"+
		"\u0001\u0000\u0000\u0000\u00ce\u00d1\u0001\u0000\u0000\u0000\u00cf\u00cd"+
		"\u0001\u0000\u0000\u0000\u00cf\u00d0\u0001\u0000\u0000\u0000\u00d0\u00d2"+
		"\u0001\u0000\u0000\u0000\u00d1\u00cf\u0001\u0000\u0000\u0000\u00d2\u00d3"+
		"\u0005\u0004\u0000\u0000\u00d3\u0013\u0001\u0000\u0000\u0000\u00d4\u00d7"+
		"\u0005\u000e\u0000\u0000\u00d5\u00d7\u0003d2\u0000\u00d6\u00d4\u0001\u0000"+
		"\u0000\u0000\u00d6\u00d5\u0001\u0000\u0000\u0000\u00d7\u00d8\u0001\u0000"+
		"\u0000\u0000\u00d8\u00dc\u0005\n\u0000\u0000\u00d9\u00da\u0005T\u0000"+
		"\u0000\u00da\u00dd\u0005\u000f\u0000\u0000\u00db\u00dd\u00038\u001c\u0000"+
		"\u00dc\u00d9\u0001\u0000\u0000\u0000\u00dc\u00db\u0001\u0000\u0000\u0000"+
		"\u00dd\u00de\u0001\u0000\u0000\u0000\u00de\u00df\u0005\u0006\u0000\u0000"+
		"\u00df\u0015\u0001\u0000\u0000\u0000\u00e0\u00e1\u0005\u0010\u0000\u0000"+
		"\u00e1\u00e5\u0005\u0011\u0000\u0000\u00e2\u00e4\u0003\u0018\f\u0000\u00e3"+
		"\u00e2\u0001\u0000\u0000\u0000\u00e4\u00e7\u0001\u0000\u0000\u0000\u00e5"+
		"\u00e3\u0001\u0000\u0000\u0000\u00e5\u00e6\u0001\u0000\u0000\u0000\u00e6"+
		"\u00e8\u0001\u0000\u0000\u0000\u00e7\u00e5\u0001\u0000\u0000\u0000\u00e8"+
		"\u00e9\u0005\u0012\u0000\u0000\u00e9\u0017\u0001\u0000\u0000\u0000\u00ea"+
		"\u00ed\u0003d2\u0000\u00eb\u00ec\u0005\u0013\u0000\u0000\u00ec\u00ee\u0003"+
		"8\u001c\u0000\u00ed\u00eb\u0001\u0000\u0000\u0000\u00ed\u00ee\u0001\u0000"+
		"\u0000\u0000\u00ee\u00ef\u0001\u0000\u0000\u0000\u00ef\u00f0\u0005\n\u0000"+
		"\u0000\u00f0\u00f5\u0005\u0002\u0000\u0000\u00f1\u00f4\u0003\u001a\r\u0000"+
		"\u00f2\u00f4\u0003\u001c\u000e\u0000\u00f3\u00f1\u0001\u0000\u0000\u0000"+
		"\u00f3\u00f2\u0001\u0000\u0000\u0000\u00f4\u00f7\u0001\u0000\u0000\u0000"+
		"\u00f5\u00f3\u0001\u0000\u0000\u0000\u00f5\u00f6\u0001\u0000\u0000\u0000"+
		"\u00f6\u00f8\u0001\u0000\u0000\u0000\u00f7\u00f5\u0001\u0000\u0000\u0000"+
		"\u00f8\u00f9\u0005\u0004\u0000\u0000\u00f9\u0019\u0001\u0000\u0000\u0000"+
		"\u00fa\u00fb\u0003b1\u0000\u00fb\u00fc\u0005\n\u0000\u0000\u00fc\u00fd"+
		"\u00038\u001c\u0000\u00fd\u00fe\u0005\u0006\u0000\u0000\u00fe\u001b\u0001"+
		"\u0000\u0000\u0000\u00ff\u0100\u0007\u0000\u0000\u0000\u0100\u0104\u0005"+
		"\u0002\u0000\u0000\u0101\u0103\u0003\u001e\u000f\u0000\u0102\u0101\u0001"+
		"\u0000\u0000\u0000\u0103\u0106\u0001\u0000\u0000\u0000\u0104\u0102\u0001"+
		"\u0000\u0000\u0000\u0104\u0105\u0001\u0000\u0000\u0000\u0105\u0107\u0001"+
		"\u0000\u0000\u0000\u0106\u0104\u0001\u0000\u0000\u0000\u0107\u0108\u0005"+
		"\u0004\u0000\u0000\u0108\u001d\u0001\u0000\u0000\u0000\u0109\u0113\u0005"+
		"\r\u0000\u0000\u010a\u010f\u0003b1\u0000\u010b\u010c\u0005\u0003\u0000"+
		"\u0000\u010c\u010e\u0003b1\u0000\u010d\u010b\u0001\u0000\u0000\u0000\u010e"+
		"\u0111\u0001\u0000\u0000\u0000\u010f\u010d\u0001\u0000\u0000\u0000\u010f"+
		"\u0110\u0001\u0000\u0000\u0000\u0110\u0114\u0001\u0000\u0000\u0000\u0111"+
		"\u010f\u0001\u0000\u0000\u0000\u0112\u0114\u0005\u0017\u0000\u0000\u0113"+
		"\u010a\u0001\u0000\u0000\u0000\u0113\u0112\u0001\u0000\u0000\u0000\u0114"+
		"\u0115\u0001\u0000\u0000\u0000\u0115\u0116\u0003\u0012\t\u0000\u0116\u001f"+
		"\u0001\u0000\u0000\u0000\u0117\u0119\u0003\"\u0011\u0000\u0118\u0117\u0001"+
		"\u0000\u0000\u0000\u0118\u0119\u0001\u0000\u0000\u0000\u0119\u011a\u0001"+
		"\u0000\u0000\u0000\u011a\u011b\u0005\u0018\u0000\u0000\u011b\u011c\u0005"+
		"\u0019\u0000\u0000\u011c\u011d\u0003$\u0012\u0000\u011d\u011e\u0005\u001a"+
		"\u0000\u0000\u011e\u0123\u0003d2\u0000\u011f\u0120\u0005\n\u0000\u0000"+
		"\u0120\u0124\u00038\u001c\u0000\u0121\u0122\u0005\u001b\u0000\u0000\u0122"+
		"\u0124\u0003b1\u0000\u0123\u011f\u0001\u0000\u0000\u0000\u0123\u0121\u0001"+
		"\u0000\u0000\u0000\u0123\u0124\u0001\u0000\u0000\u0000\u0124\u0125\u0001"+
		"\u0000\u0000\u0000\u0125\u0126\u0005\u0006\u0000\u0000\u0126!\u0001\u0000"+
		"\u0000\u0000\u0127\u0128\u0007\u0001\u0000\u0000\u0128#\u0001\u0000\u0000"+
		"\u0000\u0129\u012f\u0003d2\u0000\u012a\u012b\u0005\u0011\u0000\u0000\u012b"+
		"\u012c\u0003d2\u0000\u012c\u012d\u0005\u0012\u0000\u0000\u012d\u012f\u0001"+
		"\u0000\u0000\u0000\u012e\u0129\u0001\u0000\u0000\u0000\u012e\u012a\u0001"+
		"\u0000\u0000\u0000\u012f%\u0001\u0000\u0000\u0000\u0130\u0132\u0005\u001d"+
		"\u0000\u0000\u0131\u0130\u0001\u0000\u0000\u0000\u0131\u0132\u0001\u0000"+
		"\u0000\u0000\u0132\u0133\u0001\u0000\u0000\u0000\u0133\u0134\u0005\u001e"+
		"\u0000\u0000\u0134\u014a\u0003d2\u0000\u0135\u0136\u0005\u001b\u0000\u0000"+
		"\u0136\u014b\u0003b1\u0000\u0137\u0143\u0005\u001f\u0000\u0000\u0138\u013d"+
		"\u0003(\u0014\u0000\u0139\u013a\u0005\u0003\u0000\u0000\u013a\u013c\u0003"+
		"(\u0014\u0000\u013b\u0139\u0001\u0000\u0000\u0000\u013c\u013f\u0001\u0000"+
		"\u0000\u0000\u013d\u013b\u0001\u0000\u0000\u0000\u013d\u013e\u0001\u0000"+
		"\u0000\u0000\u013e\u0141\u0001\u0000\u0000\u0000\u013f\u013d\u0001\u0000"+
		"\u0000\u0000\u0140\u0142\u0005\u0003\u0000\u0000\u0141\u0140\u0001\u0000"+
		"\u0000\u0000\u0141\u0142\u0001\u0000\u0000\u0000\u0142\u0144\u0001\u0000"+
		"\u0000\u0000\u0143\u0138\u0001\u0000\u0000\u0000\u0143\u0144\u0001\u0000"+
		"\u0000\u0000\u0144\u0145\u0001\u0000\u0000\u0000\u0145\u0148\u0005 \u0000"+
		"\u0000\u0146\u0147\u0005!\u0000\u0000\u0147\u0149\u0003$\u0012\u0000\u0148"+
		"\u0146\u0001\u0000\u0000\u0000\u0148\u0149\u0001\u0000\u0000\u0000\u0149"+
		"\u014b\u0001\u0000\u0000\u0000\u014a\u0135\u0001\u0000\u0000\u0000\u014a"+
		"\u0137\u0001\u0000\u0000\u0000\u014a\u014b\u0001\u0000\u0000\u0000\u014b"+
		"\u014c\u0001\u0000\u0000\u0000\u014c\u014d\u0005\u0006\u0000\u0000\u014d"+
		"\'\u0001\u0000\u0000\u0000\u014e\u014f\u0003d2\u0000\u014f\u0150\u0005"+
		"\n\u0000\u0000\u0150\u0152\u0001\u0000\u0000\u0000\u0151\u014e\u0001\u0000"+
		"\u0000\u0000\u0151\u0152\u0001\u0000\u0000\u0000\u0152\u0153\u0001\u0000"+
		"\u0000\u0000\u0153\u0154\u0003$\u0012\u0000\u0154)\u0001\u0000\u0000\u0000"+
		"\u0155\u015a\u0003d2\u0000\u0156\u0157\u0005\n\u0000\u0000\u0157\u015b"+
		"\u00038\u001c\u0000\u0158\u0159\u0005\u001b\u0000\u0000\u0159\u015b\u0003"+
		"b1\u0000\u015a\u0156\u0001\u0000\u0000\u0000\u015a\u0158\u0001\u0000\u0000"+
		"\u0000\u015b\u015c\u0001\u0000\u0000\u0000\u015c\u015d\u0005\u0006\u0000"+
		"\u0000\u015d+\u0001\u0000\u0000\u0000\u015e\u016b\u0003d2\u0000\u015f"+
		"\u0168\u0005\u001f\u0000\u0000\u0160\u0165\u0003d2\u0000\u0161\u0162\u0005"+
		"\u0003\u0000\u0000\u0162\u0164\u0003d2\u0000\u0163\u0161\u0001\u0000\u0000"+
		"\u0000\u0164\u0167\u0001\u0000\u0000\u0000\u0165\u0163\u0001\u0000\u0000"+
		"\u0000\u0165\u0166\u0001\u0000\u0000\u0000\u0166\u0169\u0001\u0000\u0000"+
		"\u0000\u0167\u0165\u0001\u0000\u0000\u0000\u0168\u0160\u0001\u0000\u0000"+
		"\u0000\u0168\u0169\u0001\u0000\u0000\u0000\u0169\u016a\u0001\u0000\u0000"+
		"\u0000\u016a\u016c\u0005 \u0000\u0000\u016b\u015f\u0001\u0000\u0000\u0000"+
		"\u016b\u016c\u0001\u0000\u0000\u0000\u016c\u016d\u0001\u0000\u0000\u0000"+
		"\u016d\u016e\u0005\"\u0000\u0000\u016e\u0172\u0005\u0002\u0000\u0000\u016f"+
		"\u0171\u0003.\u0017\u0000\u0170\u016f\u0001\u0000\u0000\u0000\u0171\u0174"+
		"\u0001\u0000\u0000\u0000\u0172\u0170\u0001\u0000\u0000\u0000\u0172\u0173"+
		"\u0001\u0000\u0000\u0000\u0173\u0179\u0001\u0000\u0000\u0000\u0174\u0172"+
		"\u0001\u0000\u0000\u0000\u0175\u0177\u00038\u001c\u0000\u0176\u0178\u0005"+
		"\u0006\u0000\u0000\u0177\u0176\u0001\u0000\u0000\u0000\u0177\u0178\u0001"+
		"\u0000\u0000\u0000\u0178\u017a\u0001\u0000\u0000\u0000\u0179\u0175\u0001"+
		"\u0000\u0000\u0000\u0179\u017a\u0001\u0000\u0000\u0000\u017a\u017b\u0001"+
		"\u0000\u0000\u0000\u017b\u017c\u0005\u0004\u0000\u0000\u017c-\u0001\u0000"+
		"\u0000\u0000\u017d\u017e\u0003b1\u0000\u017e\u017f\u00030\u0018\u0000"+
		"\u017f\u0180\u00038\u001c\u0000\u0180\u0183\u0001\u0000\u0000\u0000\u0181"+
		"\u0183\u0003L&\u0000\u0182\u017d\u0001\u0000\u0000\u0000\u0182\u0181\u0001"+
		"\u0000\u0000\u0000\u0183\u0184\u0001\u0000\u0000\u0000\u0184\u0185\u0005"+
		"\u0006\u0000\u0000\u0185/\u0001\u0000\u0000\u0000\u0186\u0187\u0007\u0002"+
		"\u0000\u0000\u01871\u0001\u0000\u0000\u0000\u0188\u018b\u00034\u001a\u0000"+
		"\u0189\u018b\u00036\u001b\u0000\u018a\u0188\u0001\u0000\u0000\u0000\u018a"+
		"\u0189\u0001\u0000\u0000\u0000\u018a\u018b\u0001\u0000\u0000\u0000\u018b"+
		"\u018f\u0001\u0000\u0000\u0000\u018c\u018d\u0003d2\u0000\u018d\u018e\u0005"+
		"&\u0000\u0000\u018e\u0190\u0001\u0000\u0000\u0000\u018f\u018c\u0001\u0000"+
		"\u0000\u0000\u018f\u0190\u0001\u0000\u0000\u0000\u0190\u0191\u0001\u0000"+
		"\u0000\u0000\u0191\u0192\u0003d2\u0000\u0192\u0196\u0005\u0002\u0000\u0000"+
		"\u0193\u0195\u0003\u000e\u0007\u0000\u0194\u0193\u0001\u0000\u0000\u0000"+
		"\u0195\u0198\u0001\u0000\u0000\u0000\u0196\u0194\u0001\u0000\u0000\u0000"+
		"\u0196\u0197\u0001\u0000\u0000\u0000\u0197\u0199\u0001\u0000\u0000\u0000"+
		"\u0198\u0196\u0001\u0000\u0000\u0000\u0199\u019a\u0005\u0004\u0000\u0000"+
		"\u019a3\u0001\u0000\u0000\u0000\u019b\u019c\u0005\'\u0000\u0000\u019c"+
		"\u019d\u00038\u001c\u0000\u019d\u019e\u0005\n\u0000\u0000\u019e5\u0001"+
		"\u0000\u0000\u0000\u019f\u01a0\u0005(\u0000\u0000\u01a0\u01a5\u0003d2"+
		"\u0000\u01a1\u01a2\u0005\u0011\u0000\u0000\u01a2\u01a3\u0003d2\u0000\u01a3"+
		"\u01a4\u0005\u0012\u0000\u0000\u01a4\u01a6\u0001\u0000\u0000\u0000\u01a5"+
		"\u01a1\u0001\u0000\u0000\u0000\u01a5\u01a6\u0001\u0000\u0000\u0000\u01a6"+
		"\u01a7\u0001\u0000\u0000\u0000\u01a7\u01a8\u0005\u0014\u0000\u0000\u01a8"+
		"\u01a9\u00038\u001c\u0000\u01a9\u01aa\u0005\n\u0000\u0000\u01aa7\u0001"+
		"\u0000\u0000\u0000\u01ab\u01ac\u0003:\u001d\u0000\u01ac9\u0001\u0000\u0000"+
		"\u0000\u01ad\u01b3\u0003<\u001e\u0000\u01ae\u01af\u0005)\u0000\u0000\u01af"+
		"\u01b0\u00038\u001c\u0000\u01b0\u01b1\u0005\n\u0000\u0000\u01b1\u01b2"+
		"\u00038\u001c\u0000\u01b2\u01b4\u0001\u0000\u0000\u0000\u01b3\u01ae\u0001"+
		"\u0000\u0000\u0000\u01b3\u01b4\u0001\u0000\u0000\u0000\u01b4;\u0001\u0000"+
		"\u0000\u0000\u01b5\u01ba\u0003>\u001f\u0000\u01b6\u01b7\u0005*\u0000\u0000"+
		"\u01b7\u01b9\u0003>\u001f\u0000\u01b8\u01b6\u0001\u0000\u0000\u0000\u01b9"+
		"\u01bc\u0001\u0000\u0000\u0000\u01ba\u01b8\u0001\u0000\u0000\u0000\u01ba"+
		"\u01bb\u0001\u0000\u0000\u0000\u01bb=\u0001\u0000\u0000\u0000\u01bc\u01ba"+
		"\u0001\u0000\u0000\u0000\u01bd\u01c2\u0003@ \u0000\u01be\u01bf\u0005+"+
		"\u0000\u0000\u01bf\u01c1\u0003@ \u0000\u01c0\u01be\u0001\u0000\u0000\u0000"+
		"\u01c1\u01c4\u0001\u0000\u0000\u0000\u01c2\u01c0\u0001\u0000\u0000\u0000"+
		"\u01c2\u01c3\u0001\u0000\u0000\u0000\u01c3?\u0001\u0000\u0000\u0000\u01c4"+
		"\u01c2\u0001\u0000\u0000\u0000\u01c5\u01ca\u0003B!\u0000\u01c6\u01c7\u0007"+
		"\u0003\u0000\u0000\u01c7\u01c9\u0003B!\u0000\u01c8\u01c6\u0001\u0000\u0000"+
		"\u0000\u01c9\u01cc\u0001\u0000\u0000\u0000\u01ca\u01c8\u0001\u0000\u0000"+
		"\u0000\u01ca\u01cb\u0001\u0000\u0000\u0000\u01cbA\u0001\u0000\u0000\u0000"+
		"\u01cc\u01ca\u0001\u0000\u0000\u0000\u01cd\u01d2\u0003D\"\u0000\u01ce"+
		"\u01cf\u0007\u0004\u0000\u0000\u01cf\u01d1\u0003D\"\u0000\u01d0\u01ce"+
		"\u0001\u0000\u0000\u0000\u01d1\u01d4\u0001\u0000\u0000\u0000\u01d2\u01d0"+
		"\u0001\u0000\u0000\u0000\u01d2\u01d3\u0001\u0000\u0000\u0000\u01d3C\u0001"+
		"\u0000\u0000\u0000\u01d4\u01d2\u0001\u0000\u0000\u0000\u01d5\u01da\u0003"+
		"F#\u0000\u01d6\u01d7\u0007\u0005\u0000\u0000\u01d7\u01d9\u0003F#\u0000"+
		"\u01d8\u01d6\u0001\u0000\u0000\u0000\u01d9\u01dc\u0001\u0000\u0000\u0000"+
		"\u01da\u01d8\u0001\u0000\u0000\u0000\u01da\u01db\u0001\u0000\u0000\u0000"+
		"\u01dbE\u0001\u0000\u0000\u0000\u01dc\u01da\u0001\u0000\u0000\u0000\u01dd"+
		"\u01e2\u0003H$\u0000\u01de\u01df\u0007\u0006\u0000\u0000\u01df\u01e1\u0003"+
		"H$\u0000\u01e0\u01de\u0001\u0000\u0000\u0000\u01e1\u01e4\u0001\u0000\u0000"+
		"\u0000\u01e2\u01e0\u0001\u0000\u0000\u0000\u01e2\u01e3\u0001\u0000\u0000"+
		"\u0000\u01e3G\u0001\u0000\u0000\u0000\u01e4\u01e2\u0001\u0000\u0000\u0000"+
		"\u01e5\u01e6\u0007\u0007\u0000\u0000\u01e6\u01e9\u0003H$\u0000\u01e7\u01e9"+
		"\u0003J%\u0000\u01e8\u01e5\u0001\u0000\u0000\u0000\u01e8\u01e7\u0001\u0000"+
		"\u0000\u0000\u01e9I\u0001\u0000\u0000\u0000\u01ea\u01ec\u0005T\u0000\u0000"+
		"\u01eb\u01ed\u0007\b\u0000\u0000\u01ec\u01eb\u0001\u0000\u0000\u0000\u01ec"+
		"\u01ed\u0001\u0000\u0000\u0000\u01ed\u01fe\u0001\u0000\u0000\u0000\u01ee"+
		"\u01fe\u0005U\u0000\u0000\u01ef\u01fe\u0005S\u0000\u0000\u01f0\u01fe\u0005"+
		"5\u0000\u0000\u01f1\u01fe\u00056\u0000\u0000\u01f2\u01fe\u0003\\.\u0000"+
		"\u01f3\u01fe\u0003T*\u0000\u01f4\u01fe\u0003Z-\u0000\u01f5\u01fe\u0003"+
		"N\'\u0000\u01f6\u01fe\u0003P(\u0000\u01f7\u01fe\u0003L&\u0000\u01f8\u01fe"+
		"\u0003b1\u0000\u01f9\u01fa\u0005\u001f\u0000\u0000\u01fa\u01fb\u00038"+
		"\u001c\u0000\u01fb\u01fc\u0005 \u0000\u0000\u01fc\u01fe\u0001\u0000\u0000"+
		"\u0000\u01fd\u01ea\u0001\u0000\u0000\u0000\u01fd\u01ee\u0001\u0000\u0000"+
		"\u0000\u01fd\u01ef\u0001\u0000\u0000\u0000\u01fd\u01f0\u0001\u0000\u0000"+
		"\u0000\u01fd\u01f1\u0001\u0000\u0000\u0000\u01fd\u01f2\u0001\u0000\u0000"+
		"\u0000\u01fd\u01f3\u0001\u0000\u0000\u0000\u01fd\u01f4\u0001\u0000\u0000"+
		"\u0000\u01fd\u01f5\u0001\u0000\u0000\u0000\u01fd\u01f6\u0001\u0000\u0000"+
		"\u0000\u01fd\u01f7\u0001\u0000\u0000\u0000\u01fd\u01f8\u0001\u0000\u0000"+
		"\u0000\u01fd\u01f9\u0001\u0000\u0000\u0000\u01feK\u0001\u0000\u0000\u0000"+
		"\u01ff\u0200\u0003b1\u0000\u0200\u020c\u0005\u001f\u0000\u0000\u0201\u0206"+
		"\u00038\u001c\u0000\u0202\u0203\u0005\u0003\u0000\u0000\u0203\u0205\u0003"+
		"8\u001c\u0000\u0204\u0202\u0001\u0000\u0000\u0000\u0205\u0208\u0001\u0000"+
		"\u0000\u0000\u0206\u0204\u0001\u0000\u0000\u0000\u0206\u0207\u0001\u0000"+
		"\u0000\u0000\u0207\u020a\u0001\u0000\u0000\u0000\u0208\u0206\u0001\u0000"+
		"\u0000\u0000\u0209\u020b\u0005\u0003\u0000\u0000\u020a\u0209\u0001\u0000"+
		"\u0000\u0000\u020a\u020b\u0001\u0000\u0000\u0000\u020b\u020d\u0001\u0000"+
		"\u0000\u0000\u020c\u0201\u0001\u0000\u0000\u0000\u020c\u020d\u0001\u0000"+
		"\u0000\u0000\u020d\u020e\u0001\u0000\u0000\u0000\u020e\u020f\u0005 \u0000"+
		"\u0000\u020fM\u0001\u0000\u0000\u0000\u0210\u021c\u0005\u0011\u0000\u0000"+
		"\u0211\u0216\u00038\u001c\u0000\u0212\u0213\u0005\u0003\u0000\u0000\u0213"+
		"\u0215\u00038\u001c\u0000\u0214\u0212\u0001\u0000\u0000\u0000\u0215\u0218"+
		"\u0001\u0000\u0000\u0000\u0216\u0214\u0001\u0000\u0000\u0000\u0216\u0217"+
		"\u0001\u0000\u0000\u0000\u0217\u021a\u0001\u0000\u0000\u0000\u0218\u0216"+
		"\u0001\u0000\u0000\u0000\u0219\u021b\u0005\u0003\u0000\u0000\u021a\u0219"+
		"\u0001\u0000\u0000\u0000\u021a\u021b\u0001\u0000\u0000\u0000\u021b\u021d"+
		"\u0001\u0000\u0000\u0000\u021c\u0211\u0001\u0000\u0000\u0000\u021c\u021d"+
		"\u0001\u0000\u0000\u0000\u021d\u021e\u0001\u0000\u0000\u0000\u021e\u021f"+
		"\u0005\u0012\u0000\u0000\u021fO\u0001\u0000\u0000\u0000\u0220\u0221\u0003"+
		"d2\u0000\u0221\u022d\u0005\u0002\u0000\u0000\u0222\u0227\u0003R)\u0000"+
		"\u0223\u0224\u0005\u0003\u0000\u0000\u0224\u0226\u0003R)\u0000\u0225\u0223"+
		"\u0001\u0000\u0000\u0000\u0226\u0229\u0001\u0000\u0000\u0000\u0227\u0225"+
		"\u0001\u0000\u0000\u0000\u0227\u0228\u0001\u0000\u0000\u0000\u0228\u022b"+
		"\u0001\u0000\u0000\u0000\u0229\u0227\u0001\u0000\u0000\u0000\u022a\u022c"+
		"\u0005\u0003\u0000\u0000\u022b\u022a\u0001\u0000\u0000\u0000\u022b\u022c"+
		"\u0001\u0000\u0000\u0000\u022c\u022e\u0001\u0000\u0000\u0000\u022d\u0222"+
		"\u0001\u0000\u0000\u0000\u022d\u022e\u0001\u0000\u0000\u0000\u022e\u022f"+
		"\u0001\u0000\u0000\u0000\u022f\u0230\u0005\u0004\u0000\u0000\u0230Q\u0001"+
		"\u0000\u0000\u0000\u0231\u0232\u0003d2\u0000\u0232\u0233\u0005\n\u0000"+
		"\u0000\u0233\u0234\u00038\u001c\u0000\u0234S\u0001\u0000\u0000\u0000\u0235"+
		"\u0236\u00057\u0000\u0000\u0236\u0237\u0005\u001f\u0000\u0000\u0237\u023a"+
		"\u0005U\u0000\u0000\u0238\u0239\u0005\u0003\u0000\u0000\u0239\u023b\u0003"+
		"V+\u0000\u023a\u0238\u0001\u0000\u0000\u0000\u023a\u023b\u0001\u0000\u0000"+
		"\u0000\u023b\u023c\u0001\u0000\u0000\u0000\u023c\u023d\u0005 \u0000\u0000"+
		"\u023dU\u0001\u0000\u0000\u0000\u023e\u023f\u0005V\u0000\u0000\u023f\u0241"+
		"\u0005\u001f\u0000\u0000\u0240\u0242\u0003X,\u0000\u0241\u0240\u0001\u0000"+
		"\u0000\u0000\u0242\u0243\u0001\u0000\u0000\u0000\u0243\u0241\u0001\u0000"+
		"\u0000\u0000\u0243\u0244\u0001\u0000\u0000\u0000\u0244\u0245\u0001\u0000"+
		"\u0000\u0000\u0245\u0246\u0005 \u0000\u0000\u0246W\u0001\u0000\u0000\u0000"+
		"\u0247\u0248\u0005T\u0000\u0000\u0248Y\u0001\u0000\u0000\u0000\u0249\u024a"+
		"\u00058\u0000\u0000\u024a\u024b\u0005\u001f\u0000\u0000\u024b\u024c\u0005"+
		"U\u0000\u0000\u024c\u024d\u0005 \u0000\u0000\u024d[\u0001\u0000\u0000"+
		"\u0000\u024e\u024f\u00059\u0000\u0000\u024f\u0250\u0005\u001f\u0000\u0000"+
		"\u0250\u0255\u0003^/\u0000\u0251\u0252\u0005:\u0000\u0000\u0252\u0253"+
		"\u0005\u001f\u0000\u0000\u0253\u0255\u0005;\u0000\u0000\u0254\u024e\u0001"+
		"\u0000\u0000\u0000\u0254\u0251\u0001\u0000\u0000\u0000\u0255\u0256\u0001"+
		"\u0000\u0000\u0000\u0256\u0257\u0005\u0003\u0000\u0000\u0257\u025c\u0003"+
		"`0\u0000\u0258\u0259\u0005\u0003\u0000\u0000\u0259\u025b\u0003`0\u0000"+
		"\u025a\u0258\u0001\u0000\u0000\u0000\u025b\u025e\u0001\u0000\u0000\u0000"+
		"\u025c\u025a\u0001\u0000\u0000\u0000\u025c\u025d\u0001\u0000\u0000\u0000"+
		"\u025d\u025f\u0001\u0000\u0000\u0000\u025e\u025c\u0001\u0000\u0000\u0000"+
		"\u025f\u0260\u0005 \u0000\u0000\u0260]\u0001\u0000\u0000\u0000\u0261\u0263"+
		"\u0007\u0005\u0000\u0000\u0262\u0261\u0001\u0000\u0000\u0000\u0262\u0263"+
		"\u0001\u0000\u0000\u0000\u0263\u0264\u0001\u0000\u0000\u0000\u0264\u0265"+
		"\u0005T\u0000\u0000\u0265\u0266\u0005<\u0000\u0000\u0266_\u0001\u0000"+
		"\u0000\u0000\u0267\u026a\u00038\u001c\u0000\u0268\u0269\u0005T\u0000\u0000"+
		"\u0269\u026b\u00053\u0000\u0000\u026a\u0268\u0001\u0000\u0000\u0000\u026a"+
		"\u026b\u0001\u0000\u0000\u0000\u026ba\u0001\u0000\u0000\u0000\u026c\u0270"+
		"\u0005=\u0000\u0000\u026d\u0270\u0005>\u0000\u0000\u026e\u0270\u0003d"+
		"2\u0000\u026f\u026c\u0001\u0000\u0000\u0000\u026f\u026d\u0001\u0000\u0000"+
		"\u0000\u026f\u026e\u0001\u0000\u0000\u0000\u0270\u0275\u0001\u0000\u0000"+
		"\u0000\u0271\u0272\u0005?\u0000\u0000\u0272\u0274\u0003d2\u0000\u0273"+
		"\u0271\u0001\u0000\u0000\u0000\u0274\u0277\u0001\u0000\u0000\u0000\u0275"+
		"\u0273\u0001\u0000\u0000\u0000\u0275\u0276\u0001\u0000\u0000\u0000\u0276"+
		"c\u0001\u0000\u0000\u0000\u0277\u0275\u0001\u0000\u0000\u0000\u0278\u0279"+
		"\u0007\t\u0000\u0000\u0279e\u0001\u0000\u0000\u0000Ihjoq|\u0080\u008e"+
		"\u0091\u009b\u009f\u00a8\u00b2\u00be\u00c6\u00cf\u00d6\u00dc\u00e5\u00ed"+
		"\u00f3\u00f5\u0104\u010f\u0113\u0118\u0123\u012e\u0131\u013d\u0141\u0143"+
		"\u0148\u014a\u0151\u015a\u0165\u0168\u016b\u0172\u0177\u0179\u0182\u018a"+
		"\u018f\u0196\u01a5\u01b3\u01ba\u01c2\u01ca\u01d2\u01da\u01e2\u01e8\u01ec"+
		"\u01fd\u0206\u020a\u020c\u0216\u021a\u021c\u0227\u022b\u022d\u023a\u0243"+
		"\u0254\u025c\u0262\u026a\u026f\u0275";
	public static final ATN _ATN =
		new ATNDeserializer().deserialize(_serializedATN.toCharArray());
	static {
		_decisionToDFA = new DFA[_ATN.getNumberOfDecisions()];
		for (int i = 0; i < _ATN.getNumberOfDecisions(); i++) {
			_decisionToDFA[i] = new DFA(_ATN.getDecisionState(i), i);
		}
	}
}