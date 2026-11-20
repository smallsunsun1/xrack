#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

static UIAccessibilityCustomAction *ActionNamed(id element, NSString *name) {
  for (UIAccessibilityCustomAction *action in [element accessibilityCustomActions]) {
    if ([action.name isEqualToString:name]) {
      return action;
    }
  }
  return nil;
}

static BOOL Invoke(UIAccessibilityCustomAction *action) {
  UIAccessibilityCustomActionHandler handler = action.actionHandler;
  return handler != nil && handler(action);
}

static UIView *ActiveTextResponder(UIView *view, Class kind) {
  if ([view isKindOfClass:kind] && [view isFirstResponder]) {
    return view;
  }
  for (UIView *child in view.subviews) {
    UIView *responder = ActiveTextResponder(child, kind);
    if (responder != nil) {
      return responder;
    }
  }
  return nil;
}

static UITextField *ActiveTextField(UIView *view) {
  return (UITextField *)ActiveTextResponder(view, UITextField.class);
}

static UIBarButtonItem *AccessoryButton(UIResponder *input, NSString *title) {
  if (![input.inputAccessoryView isKindOfClass:UIToolbar.class]) {
    return nil;
  }
  for (UIBarButtonItem *item in ((UIToolbar *)input.inputAccessoryView).items) {
    if ([item.title isEqualToString:title]) {
      return item;
    }
  }
  return nil;
}

static BOOL PressAccessoryButton(UIBarButtonItem *item) {
  return item.enabled && item.action != NULL &&
      [UIApplication.sharedApplication sendAction:item.action to:item.target from:item forEvent:nil];
}

static UIScrollView *StatusBarScrollBridge(UIView *parent) {
  for (UIView *child in parent.subviews) {
    if ([child isKindOfClass:UIScrollView.class]) {
      UIScrollView *scroll = (UIScrollView *)child;
      if ([scroll.delegate isKindOfClass:NSClassFromString(@"XRackScrollToTopDelegate")]) {
        return scroll;
      }
    }
  }
  return nil;
}

@interface XRackNativeActionsTest : XCTestCase
@end

@implementation XRackNativeActionsTest

- (UIView *)waitForHost {
  NSPredicate *ready = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    UIView *host = WindowAccessibilityHost();
    return host != nil && AccessibleElement(host, @"Review options") != nil;
  }];
  [self expectationForPredicate:ready evaluatedWithObject:self handler:nil];
  [self waitForExpectationsWithTimeout:20 handler:nil];
  UIView *host = WindowAccessibilityHost();
  return AccessibleElement(host, @"Review options") != nil ? host : nil;
}

- (void)waitForLabel:(NSString *)label inHost:(UIView *)host {
  NSPredicate *visible = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, label) != nil;
  }];
  XCTNSPredicateExpectation *expectation = [[XCTNSPredicateExpectation alloc] initWithPredicate:visible object:host];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[expectation] timeout:5], XCTWaiterResultCompleted,
                 @"Expected accessible label %@", label);
}

- (void)waitForEditor:(id)element toHaveValue:(NSString *)value action:(NSString *)name {
  NSPredicate *ready = [NSPredicate predicateWithBlock:^BOOL(id editor, NSDictionary *bindings) {
    NSString *current = [editor accessibilityValue] ?: @"";
    return (value == nil || [current isEqualToString:value]) && ActionNamed(editor, name) != nil;
  }];
  XCTNSPredicateExpectation *expectation = [[XCTNSPredicateExpectation alloc] initWithPredicate:ready
                                                                                          object:element];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[expectation] timeout:5], XCTWaiterResultCompleted,
                 @"Expected value %@ and action %@", value, name);
}

- (void)testUIKitOpensTheContextMenuAndRevokesAnOldAction {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  id button = AccessibleElement(host, @"Review options");
  XCTAssertTrue([button isAccessibilityElement]);
  XCTAssertEqualObjects([button accessibilityHint], @"Record or browse an inspection review");
  XCTAssertNil(AccessibleElement(host, @"Record or browse an inspection review"));
  UIAccessibilityCustomAction *open = ActionNamed(button, @"Show context menu");
  XCTAssertTrue(Invoke(open));
  [self waitForLabel:@"Record review" inHost:host];
  XCTAssertFalse(Invoke(open));
  XCTAssertTrue([AccessibleElement(host, @"Record review") accessibilityActivate]);
  [self waitForLabel:@"Reviews 1" inHost:host];
  XCTAssertEqual(button, AccessibleElement(host, @"Review options"));
  XCTAssertEqualObjects([button accessibilityHint], @"Choose how to follow up on the review");
  XCTAssertNil(AccessibleElement(host, @"Record review"));
}

- (void)testUIKitPagesTheOpenChoicePopupWithoutSelectingUntilActivation {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Inspect popup choices" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Inspect popup choices") accessibilityActivate]);
  [self waitForLabel:@"Inspection category" inHost:host];
  id category = AccessibleElement(host, @"Inspection category");
  XCTAssertTrue([category accessibilityActivate]);
  [self waitForLabel:@"Boilers" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Valves"));
  XCTAssertTrue([AccessibleElement(host, @"Boilers") accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  [self waitForLabel:@"Valves" inHost:host];
  XCTAssertEqualObjects([AccessibleElement(host, @"Inspection category") accessibilityValue], @"Boilers");
  XCTAssertTrue([AccessibleElement(host, @"Valves") accessibilityActivate]);
  NSPredicate *selected = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return [[AccessibleElement(view, @"Inspection category") accessibilityValue] isEqualToString:@"Valves"];
  }];
  [self expectationForPredicate:selected evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertNil(AccessibleElement(host, @"Valves"));
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitActivatesAnEditableFieldAndCanReopenItsKeyboard {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Configure keyboard assistance" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Configure keyboard assistance") accessibilityActivate]);
  [self waitForLabel:@"Review note" inHost:host];
  XCTAssertNotNil([self waitForInputPurpose:nil inHost:host]);

  id review = AccessibleElement(host, @"Review note");
  XCTAssertTrue([review accessibilityActivate], @"The projected text area should accept UIKit activation.");
  NSPredicate *editingNote = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return ActiveTextResponder(view.window, UITextView.class) != nil;
  }];
  [self expectationForPredicate:editingNote evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  UITextView *note = (UITextView *)ActiveTextResponder(host.window, UITextView.class);
  NSString *original = note.text ?: @"";
  note.selectedRange = NSMakeRange(0, original.length);
  [note insertText:@"Checked"];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"accessibilityValue == %@", @"Checked"]
           evaluatedWithObject:review handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  note.selectedRange = NSMakeRange(1, 4);
  [self waitForEditor:review toHaveValue:@"Checked" action:@"Copy"];
  XCTAssertTrue([note resignFirstResponder]);
  NSPredicate *editingDismissed = [NSPredicate predicateWithBlock:^BOOL(UITextView *input, NSDictionary *bindings) {
    return !input.isFirstResponder && input.inputAccessoryView == nil;
  }];
  [self expectationForPredicate:editingDismissed evaluatedWithObject:note handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue([AccessibleElement(host, @"Review note") accessibilityActivate]);
  [self expectationForPredicate:editingNote evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertEqualObjects(note.text, @"Checked");
  XCTAssertTrue(NSEqualRanges(note.selectedRange, NSMakeRange(1, 4)));

  note.selectedRange = NSMakeRange(0, note.text.length);
  if (original.length == 0) {
    [note deleteBackward];
  } else {
    [note insertText:original];
  }
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(id element, NSDictionary *bindings) {
    return [([element accessibilityValue] ?: @"") isEqualToString:original];
  }] evaluatedWithObject:review handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
  XCTAssertFalse([review accessibilityActivate], @"A dismissed field must not regain native focus.");
}

- (void)testUIKitAppliesKeyboardAssistanceWithoutInterruptingMarkedText {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Configure keyboard assistance" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Configure keyboard assistance") accessibilityActivate]);
  [self waitForLabel:@"Use identifier keyboard" inHost:host];

  UITextField *field = [self waitForInputPurpose:nil inHost:host];
  XCTAssertNotNil(field);
  XCTAssertEqual(field.autocapitalizationType, UITextAutocapitalizationTypeWords);
  XCTAssertEqual(field.autocorrectionType, UITextAutocorrectionTypeYes);
  XCTAssertEqual(field.spellCheckingType, UITextSpellCheckingTypeYes);
  UIBarButtonItem *next = AccessoryButton(field, @"Next field");
  XCTAssertTrue(next.enabled);
  [field setMarkedText:@"shi" selectedRange:NSMakeRange(3, 0)];
  XCTAssertNotNil(field.markedTextRange);
  NSString *composed = field.text;
  XCTAssertTrue([AccessibleElement(host, @"Use identifier keyboard") accessibilityActivate]);
  [self waitForLabel:@"Use prose keyboard" inHost:host];
  XCTAssertNil(ActionNamed(AccessibleElement(host, @"Draft title"), @"Paste"));
  XCTAssertEqual(ActiveTextField(host.window), field);
  XCTAssertNotNil(field.markedTextRange);
  XCTAssertEqual(field.autocapitalizationType, UITextAutocapitalizationTypeWords);
  XCTAssertFalse(next.enabled);
  [field unmarkText];
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(UITextField *input, NSDictionary *bindings) {
    return input.autocapitalizationType == UITextAutocapitalizationTypeAllCharacters &&
           input.autocorrectionType == UITextAutocorrectionTypeNo &&
           input.spellCheckingType == UITextSpellCheckingTypeNo;
  }] evaluatedWithObject:field handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertEqualObjects(field.text, composed);
  XCTAssertTrue(next.enabled);

  XCTAssertFalse([field.delegate textFieldShouldReturn:field]);
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    UIView *editor = ActiveTextResponder(view.window, UITextView.class);
    return editor.inputAccessoryView.window != nil;
  }] evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  UITextView *note = (UITextView *)ActiveTextResponder(host.window, UITextView.class);
  XCTAssertEqual(note.autocapitalizationType, UITextAutocapitalizationTypeNone);
  XCTAssertEqual(note.autocorrectionType, UITextAutocorrectionTypeNo);
  XCTAssertEqual(note.spellCheckingType, UITextSpellCheckingTypeYes);
  XCTAssertEqual(note.returnKeyType, UIReturnKeyDefault);
  XCTAssertNil(field.inputAccessoryView);
  UIBarButtonItem *previous = AccessoryButton(note, @"Previous field");
  next = AccessoryButton(note, @"Next field");
  XCTAssertTrue(previous.enabled);
  XCTAssertTrue(next.enabled);

  id review = AccessibleElement(host, @"Review note");
  [note insertText:@"x"];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"accessibilityValue == %@", @"x"]
           evaluatedWithObject:review handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue(PressAccessoryButton(next));
  XCTestExpectation *focusMoved = [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(UITextView *input, NSDictionary *bindings) {
    return !input.isFirstResponder;
  }] evaluatedWithObject:note handler:nil];
  focusMoved.inverted = YES;
  [self waitForExpectationsWithTimeout:0.5 handler:nil];
  XCTAssertEqualObjects(note.text, @"x");

  [note insertText:@"\n"];
  [note insertText:@"y"];
  [self expectationForPredicate:[NSPredicate predicateWithFormat:@"accessibilityValue == %@", @"x\ny"]
           evaluatedWithObject:review handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue(PressAccessoryButton(previous));
  field = [self waitForInputPurpose:nil inHost:host];
  XCTAssertEqualObjects(field.text, composed);
  XCTAssertNil(note.inputAccessoryView);
  XCTAssertNotNil(AccessibleElement(host, @"Save drafts"));

  XCTAssertFalse([field.delegate textFieldShouldReturn:field]);
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(UITextView *input, NSDictionary *bindings) {
    return input.isFirstResponder && [input.text isEqualToString:@"x\ny"];
  }] evaluatedWithObject:note handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue(PressAccessoryButton(AccessoryButton(note, @"Next field")));
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return ActiveTextField(view.window).isSecureTextEntry;
  }] evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  field = ActiveTextField(host.window);
  XCTAssertEqual(field.autocapitalizationType, UITextAutocapitalizationTypeNone);
  XCTAssertEqual(field.autocorrectionType, UITextAutocorrectionTypeNo);
  XCTAssertEqual(field.spellCheckingType, UITextSpellCheckingTypeNo);
  XCTAssertEqual(field.returnKeyType, UIReturnKeyDone);
  XCTAssertNil(field.inputAccessoryView);
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitTextEditingActionsUseTheCurrentSelectionAndHistory {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Configure keyboard assistance" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Configure keyboard assistance") accessibilityActivate]);
  [self waitForLabel:@"Draft title" inHost:host];
  UITextField *field = [self waitForInputPurpose:nil inHost:host];
  XCTAssertNotNil(field);
  id title = AccessibleElement(host, @"Draft title");
  XCTAssertTrue(Invoke(ActionNamed(title, @"Select All")));
  [self waitForEditor:title toHaveValue:field.text action:@"Copy"];
  [field insertText:@"A🧪B"];
  [self waitForEditor:title toHaveValue:@"A🧪B" action:@"Select All"];
  XCTAssertTrue(Invoke(ActionNamed(title, @"Select All")));
  [self waitForEditor:title toHaveValue:@"A🧪B" action:@"Cut"];
  XCTAssertEqualObjects([field textInRange:field.selectedTextRange], @"A🧪B");

  UIAccessibilityCustomAction *copy = ActionNamed(title, @"Copy");
  NSInteger clipboardRevision = UIPasteboard.generalPasteboard.changeCount;
  XCTAssertTrue(Invoke(copy));
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(UIPasteboard *board, NSDictionary *bindings) {
    return board.changeCount > clipboardRevision;
  }] evaluatedWithObject:UIPasteboard.generalPasteboard handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertEqualObjects(UIPasteboard.generalPasteboard.string, @"A🧪B");

  XCTAssertTrue(Invoke(ActionNamed(title, @"Cut")));
  [self waitForEditor:title toHaveValue:@"" action:@"Undo"];
  XCTAssertNil(ActionNamed(title, @"Copy"));
  XCTAssertFalse(Invoke(copy));
  XCTAssertTrue(Invoke(ActionNamed(title, @"Undo")));
  [self waitForEditor:title toHaveValue:@"A🧪B" action:@"Redo"];
  XCTAssertTrue(Invoke(ActionNamed(title, @"Redo")));
  [self waitForEditor:title toHaveValue:@"" action:@"Paste"];
  XCTAssertTrue(Invoke(ActionNamed(title, @"Paste")));
  [self waitForEditor:title toHaveValue:@"A🧪B" action:@"Undo"];

  UIAccessibilityCustomAction *paste = ActionNamed(title, @"Paste");
  XCTAssertTrue([AccessibleElement(host, @"Edit access code") accessibilityActivate]);
  id password = AccessibleElement(host, @"Access code");
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(id editor, NSDictionary *bindings) {
    return ActionNamed(editor, @"Paste") != nil;
  }] evaluatedWithObject:password handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertFalse(Invoke(paste));
  [ActiveTextField(host.window) insertText:@"1357"];
  [self waitForEditor:password toHaveValue:nil action:@"Select All"];
  XCTAssertTrue(Invoke(ActionNamed(password, @"Select All")));
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(id editor, NSDictionary *bindings) {
    return ActionNamed(editor, @"Select All") == nil && ActionNamed(editor, @"Paste") != nil;
  }] evaluatedWithObject:password handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertFalse([[password accessibilityValue] isEqualToString:@"1357"]);
  XCTAssertNil(ActionNamed(password, @"Copy"));
  XCTAssertNil(ActionNamed(password, @"Cut"));
  UIPasteboard.generalPasteboard.items = @[];
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testDockedKeyboardKeepsTheCaretVisibleInALongNote {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  [self addTeardownBlock:^{
    if (AccessibleElement(host, @"Review options") == nil) {
      [host accessibilityPerformEscape];
    }
  }];
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Write long note" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Write long note") accessibilityActivate]);
  [self waitForLabel:@"Ready to write" inHost:host];
  NSPredicate *keyboardDocked = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    UIView *editor = ActiveTextResponder(view.window, UITextView.class);
    CGRect keyboard = editor.superview.keyboardLayoutGuide.layoutFrame;
    return keyboard.size.height > 120 && keyboard.size.width >= editor.superview.bounds.size.width - 1;
  }];
  XCTNSPredicateExpectation *docked = [[XCTNSPredicateExpectation alloc] initWithPredicate:keyboardDocked object:host];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[docked] timeout:5], XCTWaiterResultCompleted,
                 @"Expected the multiline editor to open the docked keyboard");
  UITextView *note = (UITextView *)ActiveTextResponder(host.window, UITextView.class);
  XCTAssertNotNil(note);

  NSMutableArray<NSString *> *lines = [NSMutableArray array];
  for (NSUInteger line = 1; line <= 17; ++line) {
    [lines addObject:[NSString stringWithFormat:@"Inspection step %lu", (unsigned long)line]];
  }
  NSString *draft = [lines componentsJoinedByString:@"\n"];
  [note insertText:draft];
  [self waitForLabel:@"Showing later lines" inHost:host];
  XCTAssertTrue(note.isFirstResponder);
  XCTAssertEqualObjects(note.text, draft);
  XCTAssertEqual(note.selectedRange.location, draft.length);

  UITextPosition *start = note.beginningOfDocument;
  note.selectedTextRange = [note textRangeFromPosition:start toPosition:start];
  [self waitForLabel:@"Showing first lines" inHost:host];
  XCTAssertTrue(note.isFirstResponder);
  XCTAssertEqualObjects(note.text, draft);
  XCTAssertTrue([AccessibleElement(host, @"Inspection narrative text") accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  [self waitForLabel:@"Showing later lines" inHost:host];
  XCTestExpectation *recentered = [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Showing first lines") != nil;
  }] evaluatedWithObject:host handler:nil];
  recentered.inverted = YES;
  [self waitForExpectationsWithTimeout:0.5 handler:nil];
  XCTAssertEqual(note.selectedRange.location, 0u);

  [note insertText:@"Checked "];
  NSString *updated = [@"Checked " stringByAppendingString:draft];
  [self expectationForPredicate:[NSPredicate predicateWithBlock:^BOOL(UITextView *editor, NSDictionary *bindings) {
    id accessible = AccessibleElement(host, @"Inspection narrative text");
    return editor.selectedRange.location == 8 && [[accessible accessibilityValue] isEqual:updated];
  }] evaluatedWithObject:note handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [self waitForLabel:@"Showing first lines" inHost:host];
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitTraversesLoadedControlsAndReturnsThroughTheActiveStack {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Test accessible navigation" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Test accessible navigation") accessibilityActivate]);

  [self waitForLabel:@"Record pressure" inHost:host];
  id pressure = AccessibleElement(host, @"Record pressure");
  XCTAssertTrue([pressure accessibilityActivate]);
  [self waitForLabel:@"Pressure recorded" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Load flow tool") accessibilityActivate]);
  [self waitForLabel:@"Record flow" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Record pressure"));
  XCTAssertFalse([pressure accessibilityActivate]);
  XCTAssertTrue([AccessibleElement(host, @"Record flow") accessibilityActivate]);
  [self waitForLabel:@"Flow recorded" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Unload tool") accessibilityActivate]);
  [self waitForLabel:@"Tool null" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Record flow"));

  XCTAssertFalse([host accessibilityPerformEscape]);
  XCTAssertTrue([AccessibleElement(host, @"Open inspection detail") accessibilityActivate]);
  [self waitForLabel:@"Lock return" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Open inspection detail"));
  XCTAssertTrue([AccessibleElement(host, @"Lock return") accessibilityActivate]);
  [self waitForLabel:@"Unlock return" inHost:host];
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Blocked returns 1" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Page detail"));

  XCTAssertTrue([AccessibleElement(host, @"Open review overlay") accessibilityActivate]);
  [self waitForLabel:@"Dismiss review overlay" inHost:host];
  XCTAssertFalse([host accessibilityPerformEscape]);
  XCTAssertTrue([AccessibleElement(host, @"Dismiss review overlay") accessibilityActivate]);
  [self waitForLabel:@"Unlock return" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Unlock return") accessibilityActivate]);
  [self waitForLabel:@"Lock return" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Open review overlay") accessibilityActivate]);
  [self waitForLabel:@"Dismiss review overlay" inHost:host];
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Lock return" inHost:host];
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Open inspection detail" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Lock return"));
  XCTAssertFalse([host accessibilityPerformEscape]);
  XCTAssertTrue([AccessibleElement(host, @"Close navigation demo") accessibilityActivate]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitMovesTheSameTableRowAndUpdatesItsBoundaryActions {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  id clear = AccessibleElement(host, @"Clear sort");
  if (([clear accessibilityTraits] & UIAccessibilityTraitNotEnabled) == 0) {
    XCTAssertTrue([clear accessibilityActivate]);
    [self waitForValue:@"Unsorted" ofControl:@"Instrument" inHost:host];
  }
  id row = FindAccessibleElement(host, ^BOOL(id element) {
    return ActionNamed(element, @"Move row up") != nil &&
           ActionNamed(element, @"Move row down") != nil;
  });
  XCTAssertNotNil(row);
  if (row == nil) {
    return;
  }
  UIAccessibilityCustomAction *moveUp = ActionNamed(row, @"Move row up");
  NSString *rowIdentifier = [row accessibilityIdentifier];
  XCTAssertNotNil(rowIdentifier);
  XCTAssertTrue(Invoke(moveUp));
  [self waitForLabel:@"Moved row 2 to 1" inHost:host];
  XCTAssertFalse(Invoke(moveUp));

  id movedRow = FindAccessibleElement(host, ^BOOL(id element) {
    return [[element accessibilityIdentifier] isEqualToString:rowIdentifier];
  });
  XCTAssertNotNil(movedRow);
  XCTAssertNil(ActionNamed(movedRow, @"Move row up"));
  XCTAssertTrue(Invoke(ActionNamed(movedRow, @"Move row down")));
  [self waitForLabel:@"Moved row 1 to 2" inHost:host];
}

- (void)waitForSortValue:(NSString *)value
                  order:(NSArray<NSString *> *)order
                 inHost:(UIView *)host {
  NSPredicate *sorted = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    id header = AccessibleElement(view, @"Instrument");
    if (![[header accessibilityValue] isEqualToString:value]) {
      return NO;
    }
    CGFloat lastY = -CGFLOAT_MAX;
    for (NSString *name in order) {
      id cell = AccessibleElement(view, name);
      if (cell == nil || CGRectGetMinY([cell accessibilityFrame]) <= lastY) {
        return NO;
      }
      lastY = CGRectGetMinY([cell accessibilityFrame]);
    }
    return YES;
  }];
  [self expectationForPredicate:sorted evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (void)testUIKitSortsTheTableAndReportsTheNextDirection {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }

  id header = AccessibleElement(host, @"Instrument");
  XCTAssertTrue(([header accessibilityTraits] & UIAccessibilityTraitButton) != 0);
  XCTAssertEqualObjects([header accessibilityValue], @"Unsorted");
  XCTAssertEqualObjects([header accessibilityHint], @"Sort ascending");
  id location = AccessibleElement(host, @"Location");
  XCTAssertNotNil(location);
  XCTAssertFalse([location accessibilityActivate]);

  XCTAssertTrue([header accessibilityActivate]);
  [self waitForSortValue:@"Sorted ascending"
                   order:@[@"Gauge", @"Sensor", @"Valve"]
                  inHost:host];
  XCTAssertEqual(header, AccessibleElement(host, @"Instrument"));
  XCTAssertEqualObjects([header accessibilityHint], @"Sort descending");

  XCTAssertTrue([header accessibilityActivate]);
  [self waitForSortValue:@"Sorted descending"
                   order:@[@"Valve", @"Sensor", @"Gauge"]
                  inHost:host];
  XCTAssertEqual(header, AccessibleElement(host, @"Instrument"));
  XCTAssertEqualObjects([header accessibilityHint], @"Sort ascending");

  id clear = AccessibleElement(host, @"Clear sort");
  XCTAssertEqualObjects([clear accessibilityHint], @"Restore the original instrument order");
  XCTAssertNil(AccessibleElement(host, @"Return to the original row order"));
  XCTAssertTrue([clear accessibilityActivate]);
  [self waitForSortValue:@"Unsorted"
                   order:@[@"Gauge", @"Valve", @"Sensor"]
                  inHost:host];
}

- (void)testUIKitMovesTheSameColumnPastHiddenColumnsAndKeepsSorting {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  id instrument = AccessibleElement(host, @"Instrument");
  id location = AccessibleElement(host, @"Location");
  XCTAssertNotNil(instrument);
  XCTAssertNotNil(location);
  XCTAssertNil(AccessibleElement(host, @"Internal name"));
  XCTAssertNil(ActionNamed(instrument, @"Move column left"));
  XCTAssertNotNil(ActionNamed(location, @"Move column left"));
  UIAccessibilityCustomAction *right = ActionNamed(instrument, @"Move column right");
  XCTAssertTrue(Invoke(right));
  [self waitForLabel:@"Moved column 1 to 3" inHost:host];

  XCTAssertEqual(instrument, AccessibleElement(host, @"Instrument"));
  XCTAssertTrue(CGRectGetMinX([location accessibilityFrame]) <
                CGRectGetMinX([instrument accessibilityFrame]));
  XCTAssertNil(ActionNamed(instrument, @"Move column right"));
  XCTAssertFalse(Invoke(right));
  XCTAssertTrue([instrument accessibilityActivate]);
  [self waitForSortValue:@"Sorted ascending" order:@[@"Gauge", @"Sensor", @"Valve"] inHost:host];

  XCTAssertTrue(Invoke(ActionNamed(instrument, @"Move column left")));
  [self waitForLabel:@"Moved column 1 to 2" inHost:host];
  XCTAssertEqual(instrument, AccessibleElement(host, @"Instrument"));
  XCTAssertNil(ActionNamed(instrument, @"Move column left"));
  XCTAssertTrue(CGRectGetMinX([instrument accessibilityFrame]) <
                CGRectGetMinX([location accessibilityFrame]));
  XCTAssertEqualObjects([instrument accessibilityValue], @"Sorted ascending");
}

- (void)testUIKitResizesTableHeadersWithinLimitsAndKeepsTheSameRecord {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  id clear = AccessibleElement(host, @"Clear sort");
  if (([clear accessibilityTraits] & UIAccessibilityTraitNotEnabled) == 0) {
    XCTAssertTrue([clear accessibilityActivate]);
    [self waitForValue:@"Unsorted" ofControl:@"Instrument" inHost:host];
  }
  XCTAssertTrue([AccessibleElement(host, @"Resize table headers") accessibilityActivate]);
  id instrument = [self waitForValue:@"Width 120 points. Unsorted" ofControl:@"Instrument" inHost:host];
  XCTAssertNil(ActionNamed(AccessibleElement(host, @"Location"), @"Increase column width"));
  XCTAssertNotNil(ActionNamed(instrument, @"Move column right"));
  CGFloat baselineWidth = CGRectGetWidth([instrument accessibilityFrame]);
  UIAccessibilityCustomAction *increase = ActionNamed(instrument, @"Increase column width");
  XCTAssertTrue(Invoke(increase));
  XCTAssertEqual(instrument, [self waitForValue:@"Width 124 points. Unsorted"
                                    ofControl:@"Instrument" inHost:host]);
  XCTAssertEqualWithAccuracy(CGRectGetWidth([instrument accessibilityFrame]) - baselineWidth, 4, 0.5);
  XCTAssertNil(ActionNamed(instrument, @"Increase column width"));
  XCTAssertFalse(Invoke(increase));
  XCTAssertTrue(Invoke(ActionNamed(instrument, @"Decrease column width")));
  [self waitForValue:@"Width 116 points. Unsorted" ofControl:@"Instrument" inHost:host];
  UIAccessibilityCustomAction *decrease = ActionNamed(instrument, @"Decrease column width");
  XCTAssertTrue(Invoke(decrease));
  [self waitForValue:@"Width 112 points. Unsorted" ofControl:@"Instrument" inHost:host];
  XCTAssertNil(ActionNamed(instrument, @"Decrease column width"));
  XCTAssertFalse(Invoke(decrease));

  id row = FindAccessibleElement(host, ^BOOL(id element) {
    return ActionNamed(element, @"Move row up") != nil && ActionNamed(element, @"Move row down") != nil;
  });
  XCTAssertNotNil(row);
  NSString *label = [row accessibilityLabel];
  CGFloat baselineHeight = CGRectGetHeight([row accessibilityFrame]);
  XCTAssertEqualObjects([row accessibilityValue], @"Height 43 points");
  XCTAssertTrue(Invoke(ActionNamed(row, @"Increase row height")));
  XCTAssertEqual(row, [self waitForValue:@"Height 51 points" ofControl:label inHost:host]);
  XCTAssertEqualWithAccuracy(CGRectGetHeight([row accessibilityFrame]) - baselineHeight, 8, 0.5);
  XCTAssertTrue(Invoke(ActionNamed(row, @"Decrease row height")));
  [self waitForValue:@"Height 43 points" ofControl:label inHost:host];
  XCTAssertTrue(Invoke(ActionNamed(row, @"Increase row height")));
  [self waitForValue:@"Height 51 points" ofControl:label inHost:host];
  NSString *rowIdentifier = [row accessibilityIdentifier];

  XCTAssertTrue([instrument accessibilityActivate]);
  XCTAssertEqual(instrument, [self waitForValue:@"Width 112 points. Sorted ascending"
                                    ofControl:@"Instrument" inHost:host]);
  id reordered = FindAccessibleElement(host, ^BOOL(id element) {
    return [[element accessibilityIdentifier] isEqualToString:rowIdentifier];
  });
  XCTAssertEqual(row, reordered);
  XCTAssertEqualObjects([reordered accessibilityValue], @"Height 51 points");
  XCTAssertTrue(Invoke(ActionNamed(reordered, @"Decrease row height")));
  [self waitForValue:@"Height 43 points" ofControl:[reordered accessibilityLabel] inHost:host];
  XCTAssertTrue(Invoke(ActionNamed(instrument, @"Increase column width")));
  [self waitForValue:@"Width 120 points. Sorted ascending" ofControl:@"Instrument" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Resize table headers") accessibilityActivate]);
  [self waitForValue:@"Sorted ascending" ofControl:@"Instrument" inHost:host];
  XCTAssertTrue([clear accessibilityActivate]);
  [self waitForValue:@"Unsorted" ofControl:@"Instrument" inHost:host];
}

- (void)waitForDraft:(NSString *)draft editorNamed:(NSString *)label inHost:(UIView *)host {
  NSPredicate *value = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    id editor = AccessibleElement(view, label);
    return [[editor accessibilityValue] isEqualToString:draft] &&
           ActiveTextField(view.window) != nil;
  }];
  [self expectationForPredicate:value evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (UITextField *)waitForInputPurpose:(UITextContentType)purpose
                            inHost:(UIView *)host {
  NSPredicate *configured = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    UITextField *field = ActiveTextField(view.window);
    return field != nil && (purpose == nil ? field.textContentType.length == 0 :
                            [field.textContentType isEqualToString:purpose]);
  }];
  [self expectationForPredicate:configured evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  return ActiveTextField(host.window);
}

- (void)testUIKitReceivesSignInPurposesAndKeepsKeyboardModesIndependent {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Sign-in fields" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Sign-in fields") accessibilityActivate]);

  UITextField *field = [self waitForInputPurpose:UITextContentTypeUsername inHost:host];
  XCTAssertEqual(field.keyboardType, UIKeyboardTypeEmailAddress);
  XCTAssertEqual(field.returnKeyType, UIReturnKeyNext);
  [field insertText:@"agent@example.test"];
  [self waitForDraft:@"agent@example.test" editorNamed:@"Account email" inHost:host];
  XCTAssertFalse([field.delegate textFieldShouldReturn:field]);

  field = [self waitForInputPurpose:UITextContentTypePassword inHost:host];
  XCTAssertTrue(field.secureTextEntry);
  [field insertText:@"sample"];
  NSPredicate *canSelect = [NSPredicate predicateWithBlock:^BOOL(UITextField *input, NSDictionary *bindings) {
    return [input canPerformAction:@selector(selectAll:) withSender:nil];
  }];
  [self expectationForPredicate:canSelect evaluatedWithObject:field handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [field selectAll:nil];
  NSPredicate *selected = [NSPredicate predicateWithBlock:^BOOL(UITextField *input, NSDictionary *bindings) {
    UITextRange *range = input.selectedTextRange;
    return range != nil && [input offsetFromPosition:range.start toPosition:range.end] == 6;
  }];
  [self expectationForPredicate:selected evaluatedWithObject:field handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  UIPasteboard *pasteboard = UIPasteboard.generalPasteboard;
  pasteboard.string = @"clipboard sentinel";
  XCTAssertFalse([field canPerformAction:@selector(copy:) withSender:nil]);
  XCTAssertFalse([field canPerformAction:@selector(cut:) withSender:nil]);
  [field copy:nil];
  [field cut:nil];
  XCTAssertEqualObjects(pasteboard.string, @"clipboard sentinel");
  XCTAssertEqualObjects(field.text, @"sample");
  pasteboard.items = @[];
  XCTAssertFalse([field.delegate textFieldShouldReturn:field]);

  field = [self waitForInputPurpose:UITextContentTypeNewPassword inHost:host];
  XCTAssertTrue(field.secureTextEntry);
  XCTAssertFalse([field.delegate textFieldShouldReturn:field]);

  field = [self waitForInputPurpose:UITextContentTypeOneTimeCode inHost:host];
  XCTAssertFalse(field.secureTextEntry);
  XCTAssertEqual(field.keyboardType, UIKeyboardTypeASCIICapableNumberPad);
  [field insertText:@"123456"];
  [self waitForDraft:@"123456" editorNamed:@"Verification code" inHost:host];
  XCTAssertFalse([field.delegate textFieldShouldReturn:field]);

  field = [self waitForInputPurpose:nil inHost:host];
  XCTAssertFalse(field.secureTextEntry);
  XCTAssertEqual(field.keyboardType, UIKeyboardTypeDefault);
  XCTAssertEqual(field.returnKeyType, UIReturnKeyDone);
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitEditsAndCancelsTheSameTableCell {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  id cell = AccessibleElement(host, @"Pump room");
  XCTAssertNotNil(cell);
  UIAccessibilityCustomAction *edit = ActionNamed(cell, @"Edit cell");
  XCTAssertTrue(Invoke(edit));
  if (edit == nil) {
    return;
  }
  [self waitForDraft:@"Pump room" editorNamed:@"Location editor" inHost:host];
  XCTAssertEqual(cell, AccessibleElement(host, @"Location editor"));
  XCTAssertFalse(Invoke(edit));
  [ActiveTextField(host.window) insertText:@"Bay 7"];
  [self waitForDraft:@"Bay 7" editorNamed:@"Location editor" inHost:host];
  XCTAssertTrue(Invoke(ActionNamed(cell, @"Save edit")));
  [self waitForLabel:@"Bay 7" inHost:host];
  XCTAssertEqual(cell, AccessibleElement(host, @"Bay 7"));
  XCTAssertNil(ActiveTextField(host.window));

  XCTAssertTrue(Invoke(ActionNamed(cell, @"Edit cell")));
  [self waitForDraft:@"Bay 7" editorNamed:@"Location editor" inHost:host];
  [ActiveTextField(host.window) insertText:@"Discard"];
  [self waitForDraft:@"Discard" editorNamed:@"Location editor" inHost:host];
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Bay 7" inHost:host];
  XCTAssertEqual(cell, AccessibleElement(host, @"Bay 7"));
  XCTAssertNil(AccessibleElement(host, @"Discard"));
  XCTAssertNil(ActiveTextField(host.window));
}

- (void)testUIKitExpandsAndCollapsesTheSameTreeBranch {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  id branch = AccessibleElement(host, @"Pump area");
  UIAccessibilityCustomAction *expand = ActionNamed(branch, @"Expand");
  XCTAssertNotNil(expand);
  XCTAssertTrue(Invoke(expand));
  if (expand == nil) {
    return;
  }
  [self waitForLabel:@"Intake valve" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Return valve"));
  XCTAssertEqual(branch, AccessibleElement(host, @"Pump area"));
  XCTAssertEqualObjects([branch accessibilityValue], @"Expanded");
  XCTAssertFalse(Invoke(expand));

  id note = AccessibleElement(host, @"Service note");
  XCTAssertTrue(Invoke(ActionNamed(note, @"Edit cell")));
  [self waitForDraft:@"Service note" editorNamed:@"Cell editor" inHost:host];
  [ActiveTextField(host.window) insertText:@"Rechecked"];
  [self waitForDraft:@"Rechecked" editorNamed:@"Cell editor" inHost:host];
  XCTAssertTrue(Invoke(ActionNamed(note, @"Save edit")));
  [self waitForLabel:@"Rechecked" inHost:host];
  XCTAssertEqual(note, AccessibleElement(host, @"Rechecked"));

  XCTAssertTrue(Invoke(ActionNamed(branch, @"Collapse")));
  NSPredicate *collapsed = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Intake valve") == nil &&
           [[AccessibleElement(view, @"Pump area") accessibilityValue] isEqualToString:@"Collapsed"];
  }];
  [self expectationForPredicate:collapsed evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertEqual(branch, AccessibleElement(host, @"Pump area"));
  XCTAssertNotNil(AccessibleElement(host, @"Tank area"));
  XCTAssertFalse(Invoke(expand));
}

- (void)testUIKitClosesBackgroundAndSelectedTabs {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Manage tabs" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Manage tabs") accessibilityActivate]);
  [self waitForLabel:@"Inspection tabs" inHost:host];

  id draft = AccessibleElement(host, @"Draft");
  XCTAssertNotNil(draft);
  XCTAssertTrue(([draft accessibilityTraits] & UIAccessibilityTraitSelected) != 0);
  id resultsClose = AccessibleElement(host, @"Close Results");
  XCTAssertNotNil(resultsClose);
  if (resultsClose == nil) {
    return;
  }
  [self waitForLabel:@"Draft content" inHost:host];
  XCTAssertTrue([resultsClose accessibilityActivate]);
  NSPredicate *removed = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Results") == nil;
  }];
  [self expectationForPredicate:removed evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertFalse([resultsClose accessibilityActivate]);
  XCTAssertEqual(draft, AccessibleElement(host, @"Draft"));
  XCTAssertTrue(([draft accessibilityTraits] & UIAccessibilityTraitSelected) != 0);

  XCTAssertTrue([AccessibleElement(host, @"Close Draft") accessibilityActivate]);
  [self waitForLabel:@"Pinned content" inHost:host];
  id pinned = AccessibleElement(host, @"Pinned");
  XCTAssertTrue(([pinned accessibilityTraits] & UIAccessibilityTraitSelected) != 0);
  XCTAssertNil(AccessibleElement(host, @"Draft"));
  XCTAssertNil(AccessibleElement(host, @"Close Pinned"));
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitScrollsTheTabStripFromATabOrCloseButton {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Browse narrow tabs" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Browse narrow tabs") accessibilityActivate]);
  [self waitForLabel:@"Close Draft" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Archive"));
  id close = AccessibleElement(host, @"Close Draft");
  XCTAssertFalse([close accessibilityScroll:UIAccessibilityScrollDirectionLeft]);
  BOOL advanced = [close accessibilityScroll:UIAccessibilityScrollDirectionRight];
  XCTAssertTrue(advanced);
  if (!advanced) {
    [host accessibilityPerformEscape];
    return;
  }
  [self waitForLabel:@"Archive" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Overview"));
  XCTAssertNotNil(AccessibleElement(host, @"Current tab: overview"));

  id archive = AccessibleElement(host, @"Archive");
  XCTAssertFalse([archive accessibilityScroll:UIAccessibilityScrollDirectionRight]);
  XCTAssertTrue([archive accessibilityScroll:UIAccessibilityScrollDirectionLeft]);
  [self waitForLabel:@"Close Draft" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Archive"));
  XCTAssertNotNil(AccessibleElement(host, @"Current tab: overview"));
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitMovesTabsAndRevealsTheSameElementWithoutSelectingIt {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Browse narrow tabs" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Browse narrow tabs") accessibilityActivate]);
  [self waitForLabel:@"Close Draft" inHost:host];
  id draft = AccessibleElement(host, @"Draft");
  XCTAssertNotNil(draft);
  XCTAssertNil(AccessibleElement(host, @"Archive"));

  XCTAssertTrue(Invoke(ActionNamed(draft, @"Move tab right")));
  [self waitForLabel:@"Moved tab 2 to 3" inHost:host];
  XCTAssertEqual(draft, AccessibleElement(host, @"Draft"));
  XCTAssertNotNil(AccessibleElement(host, @"Close Draft"));
  XCTAssertNotNil(AccessibleElement(host, @"Current tab: overview"));
  XCTAssertEqual([draft accessibilityTraits] & UIAccessibilityTraitSelected, 0);

  UIAccessibilityCustomAction *right = ActionNamed(draft, @"Move tab right");
  XCTAssertTrue(Invoke(right));
  [self waitForLabel:@"Moved tab 3 to 4" inHost:host];
  XCTAssertEqual(draft, AccessibleElement(host, @"Draft"));
  XCTAssertNotNil(AccessibleElement(host, @"Archive"));
  XCTAssertNotNil(ActionNamed(draft, @"Move tab left"));
  XCTAssertNil(ActionNamed(draft, @"Move tab right"));
  XCTAssertFalse(Invoke(right));
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];

  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Manage tabs" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Manage tabs") accessibilityActivate]);
  [self waitForLabel:@"Draft content" inHost:host];
  id selected = AccessibleElement(host, @"Draft");
  id pinned = AccessibleElement(host, @"Pinned");
  XCTAssertNotNil(selected);
  XCTAssertNotNil(pinned);
  XCTAssertTrue(Invoke(ActionNamed(selected, @"Move tab left")));
  NSPredicate *moved = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return CGRectGetMinX([selected accessibilityFrame]) < CGRectGetMinX([pinned accessibilityFrame]);
  }];
  XCTNSPredicateExpectation *position =
      [[XCTNSPredicateExpectation alloc] initWithPredicate:moved object:nil];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[position] timeout:5], XCTWaiterResultCompleted);
  XCTAssertEqual(selected, AccessibleElement(host, @"Draft"));
  XCTAssertNotEqual([selected accessibilityTraits] & UIAccessibilityTraitSelected, 0);
  XCTAssertNotNil(AccessibleElement(host, @"Draft content"));
}

- (void)testUIKitPagesFromAControlAndLeavesVerticalScrollingInItsViewport {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Inspect cards" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Inspect cards") accessibilityActivate]);
  [self waitForLabel:@"Card summary" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Card notes"));
  id summary = AccessibleElement(host, @"Card summary");
  XCTAssertFalse([summary accessibilityScroll:UIAccessibilityScrollDirectionLeft]);
  XCTAssertTrue([summary accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  [self waitForLabel:@"Card notes" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Card changes: 0"));

  id notes = AccessibleElement(host, @"Card notes");
  XCTAssertTrue([notes accessibilityScroll:UIAccessibilityScrollDirectionRight]);
  [self waitForLabel:@"Finish inspection" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Card changes: 1"));
  XCTAssertNil(AccessibleElement(host, @"Hidden card"));
  XCTAssertNil(AccessibleElement(host, @"Disabled card"));
  XCTAssertFalse([notes accessibilityScroll:UIAccessibilityScrollDirectionRight]);

  id finish = AccessibleElement(host, @"Finish inspection");
  XCTAssertFalse([finish accessibilityScroll:UIAccessibilityScrollDirectionRight]);
  XCTAssertTrue([AccessibleElement(host, @"Mirror cards") accessibilityActivate]);
  [self waitForLabel:@"Restore card direction" inHost:host];
  XCTAssertFalse([finish accessibilityScroll:UIAccessibilityScrollDirectionLeft]);
  XCTAssertTrue([finish accessibilityScroll:UIAccessibilityScrollDirectionRight]);
  [self waitForLabel:@"Card changes: 2" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Finish inspection"));
  XCTAssertTrue(AccessibleElement(host, @"Card summary") != nil || AccessibleElement(host, @"Card notes") != nil);
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitScrollsNestedViewportsFromStaticText {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Read service log" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Read service log") accessibilityActivate]);
  [self waitForLabel:@"Morning shift" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Night shift"));
  id heading = AccessibleElement(host, @"Shift notes");
  XCTAssertNotNil(heading);
  XCTAssertTrue(([heading accessibilityTraits] & UIAccessibilityTraitHeader) != 0);
  id morning = AccessibleElement(host, @"Morning shift");
  XCTAssertFalse(([morning accessibilityTraits] & UIAccessibilityTraitHeader) != 0);
  XCTAssertFalse([morning accessibilityActivate]);
  XCTAssertFalse([morning accessibilityScroll:UIAccessibilityScrollDirectionLeft]);
  BOOL advanced = [morning accessibilityScroll:UIAccessibilityScrollDirectionRight];
  XCTAssertTrue(advanced);
  if (!advanced) {
    [host accessibilityPerformEscape];
    return;
  }
  [self waitForLabel:@"Night shift" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Morning shift"));
  XCTAssertNotNil(AccessibleElement(host, @"Shift notes"));
  XCTAssertNil(AccessibleElement(host, @"Parts replaced"));

  id night = AccessibleElement(host, @"Night shift");
  XCTAssertTrue([night accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  [self waitForLabel:@"Parts replaced" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Night shift"));
  id parts = AccessibleElement(host, @"Parts replaced");
  XCTAssertTrue(([parts accessibilityTraits] & UIAccessibilityTraitHeader) != 0);
  XCTAssertFalse([parts accessibilityScroll:UIAccessibilityScrollDirectionRight]);
  XCTAssertTrue([parts accessibilityScroll:UIAccessibilityScrollDirectionDown]);
  [self waitForLabel:@"Night shift" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Morning shift"));
  XCTAssertTrue([AccessibleElement(host, @"Night shift") accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  [self waitForLabel:@"Parts replaced" inHost:host];
  UIScrollView *statusBarScroll = StatusBarScrollBridge(host.superview);
  XCTAssertTrue(statusBarScroll.scrollsToTop);
  XCTAssertFalse([statusBarScroll.delegate scrollViewShouldScrollToTop:statusBarScroll]);
  [self waitForLabel:@"Shift notes" inHost:host];
  XCTAssertTrue(([AccessibleElement(host, @"Shift notes") accessibilityTraits] &
                 UIAccessibilityTraitHeader) != 0);
  XCTAssertNotNil(AccessibleElement(host, @"Night shift"));
  XCTAssertNil(AccessibleElement(host, @"Morning shift"));
  XCTAssertNil(AccessibleElement(host, @"Parts replaced"));
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)waitForReportPage:(NSString *)value inHost:(UIView *)host {
  NSPredicate *selection = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return [[AccessibleElement(view, @"Report pages") accessibilityValue] isEqualToString:value];
  }];
  [self expectationForPredicate:selection evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (id)waitForValue:(NSString *)value ofControl:(NSString *)label inHost:(UIView *)host {
  NSPredicate *matches = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return [[AccessibleElement(view, label) accessibilityValue] isEqualToString:value];
  }];
  XCTNSPredicateExpectation *expectation = [[XCTNSPredicateExpectation alloc] initWithPredicate:matches object:host];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[expectation] timeout:5], XCTWaiterResultCompleted,
                 @"Expected %@ to report value %@", label, value);
  return AccessibleElement(host, label);
}

- (void)testUIKitAdjustsNumericControlsAndKeepsTheirIdentity {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Adjust instrument values" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Adjust instrument values") accessibilityActivate]);

  id gain = [self waitForValue:@"0" ofControl:@"Sensor gain" inHost:host];
  XCTAssertTrue(([gain accessibilityTraits] & UIAccessibilityTraitAdjustable) != 0);
  [gain accessibilityDecrement];
  [gain accessibilityIncrement];
  XCTAssertEqual(gain, [self waitForValue:@"1" ofControl:@"Sensor gain" inHost:host]);
  [gain accessibilityIncrement];
  [self waitForValue:@"2" ofControl:@"Sensor gain" inHost:host];
  [gain accessibilityIncrement];
  [gain accessibilityDecrement];
  [self waitForValue:@"1" ofControl:@"Sensor gain" inHost:host];

  id lower = [self waitForValue:@"4" ofControl:@"Lower value" inHost:host];
  id upper = [self waitForValue:@"6" ofControl:@"Upper value" inHost:host];
  XCTAssertNotEqual(lower, upper);
  [lower accessibilityIncrement];
  XCTAssertEqual(lower, [self waitForValue:@"6" ofControl:@"Lower value" inHost:host]);
  [lower accessibilityIncrement];
  [upper accessibilityIncrement];
  XCTAssertEqual(upper, [self waitForValue:@"8" ofControl:@"Upper value" inHost:host]);
  XCTAssertEqualObjects([lower accessibilityValue], @"6");

  id offset = [self waitForValue:@"0,75" ofControl:@"Calibrated offset" inHost:host];
  XCTAssertTrue(([offset accessibilityTraits] & UIAccessibilityTraitAdjustable) != 0);
  [offset accessibilityIncrement];
  XCTAssertEqual(offset, [self waitForValue:@"1,00" ofControl:@"Calibrated offset" inHost:host]);
  [offset accessibilityIncrement];
  [offset accessibilityDecrement];
  [self waitForValue:@"0,75" ofControl:@"Calibrated offset" inHost:host];

  id fixed = [self waitForValue:@"7" ofControl:@"Fixed sample count" inHost:host];
  XCTAssertEqual([fixed accessibilityTraits] & UIAccessibilityTraitAdjustable, 0);
  XCTAssertFalse([fixed accessibilityActivate]);
  [fixed accessibilityIncrement];
  XCTAssertEqualObjects([fixed accessibilityValue], @"7");

  id minute = [self waitForValue:@"59" ofControl:@"Minute" inHost:host];
  XCTAssertTrue(([minute accessibilityTraits] & UIAccessibilityTraitAdjustable) != 0);
  [minute accessibilityIncrement];
  XCTAssertEqual(minute, [self waitForValue:@"00" ofControl:@"Minute" inHost:host]);
  [minute accessibilityDecrement];
  [self waitForValue:@"59" ofControl:@"Minute" inHost:host];
  XCTAssertEqualObjects([AccessibleElement(host, @"Hour") accessibilityValue], @"14");

  XCTAssertTrue([offset accessibilityActivate]);
  UITextField *number = [self waitForInputPurpose:nil inHost:host];
  XCTAssertNotNil(number);
  XCTAssertEqual(number.keyboardType, UIKeyboardTypeDecimalPad);
  XCTAssertEqual(number.returnKeyType, UIReturnKeyDone);
  number.selectedTextRange = [number textRangeFromPosition:number.beginningOfDocument
                                                    toPosition:number.endOfDocument];
  [number insertText:@"0,50"];
  [self waitForValue:@"0,50" ofControl:@"Calibrated offset" inHost:host];
  UIToolbar *numberToolbar = (UIToolbar *)number.inputAccessoryView;
  XCTAssertTrue([numberToolbar isKindOfClass:UIToolbar.class]);
  XCTAssertTrue(PressAccessoryButton(numberToolbar.items.lastObject));
  NSPredicate *numberDismissed = [NSPredicate predicateWithBlock:^BOOL(UITextField *field, NSDictionary *bindings) {
    return !field.isFirstResponder && field.inputAccessoryView == nil;
  }];
  [self expectationForPredicate:numberDismissed evaluatedWithObject:number handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  [self waitForValue:@"0,50" ofControl:@"Calibrated offset" inHost:host];
  [offset accessibilityIncrement];
  [self waitForValue:@"0,75" ofControl:@"Calibrated offset" inHost:host];

  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitReportsLocalizedProgressAndClearsTheValueWhileIndeterminate {
  self.continueAfterFailure = NO;
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Adjust instrument values" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Adjust instrument values") accessibilityActivate]);

  NSString *label = @"Sample transfer";
  NSString *partial = [NSNumberFormatter localizedStringFromNumber:@0.35
                                                        numberStyle:NSNumberFormatterPercentStyle];
  NSString *completed = [NSNumberFormatter localizedStringFromNumber:@1.0
                                                          numberStyle:NSNumberFormatterPercentStyle];
  id transfer = [self waitForValue:partial ofControl:label inHost:host];
  XCTAssertNotEqual([transfer accessibilityTraits] & UIAccessibilityTraitUpdatesFrequently, 0);
  XCTAssertEqual([transfer accessibilityTraits] & UIAccessibilityTraitAdjustable, 0);
  XCTAssertFalse([transfer accessibilityActivate]);

  id gain = AccessibleElement(host, @"Sensor gain");
  [gain accessibilityIncrement];
  [self waitForValue:@"1" ofControl:@"Sensor gain" inHost:host];
  XCTAssertEqual(AccessibleElement(host, label), transfer);
  XCTAssertNil([transfer accessibilityValue]);

  [gain accessibilityIncrement];
  XCTAssertEqual([self waitForValue:completed ofControl:label inHost:host], transfer);
}

- (void)testUIKitAdjustsPageIndicatorInLogicalOrderAndRespectsItsBoundaries {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Report page controls" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Report page controls") accessibilityActivate]);
  [self waitForReportPage:@"Page 1 of 3" inHost:host];
  id pages = AccessibleElement(host, @"Report pages");
  XCTAssertTrue(([pages accessibilityTraits] & UIAccessibilityTraitAdjustable) != 0);
  XCTAssertNotNil(AccessibleElement(host, @"Report overview"));

  [pages accessibilityDecrement];
  [pages accessibilityIncrement];
  [self waitForReportPage:@"Page 2 of 3" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Report findings"));
  XCTAssertNotNil(AccessibleElement(host, @"Page changes: 1"));
  [pages accessibilityIncrement];
  [self waitForReportPage:@"Page 3 of 3" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Report sign-off"));
  [pages accessibilityIncrement];
  [pages accessibilityDecrement];
  [self waitForReportPage:@"Page 2 of 3" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Page changes: 3"));

  XCTAssertTrue([AccessibleElement(host, @"Lock page changes") accessibilityActivate]);
  [self waitForLabel:@"Unlock page changes" inHost:host];
  XCTAssertEqual([pages accessibilityTraits] & UIAccessibilityTraitAdjustable, 0);
  [pages accessibilityDecrement];
  [self waitForReportPage:@"Page 2 of 3" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Page changes: 3"));
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitOperatesDisclosureHeadersAndTheirChildren {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Disclosure controls" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Disclosure controls") accessibilityActivate]);
  [self waitForLabel:@"Advanced options" inHost:host];
  id status = AccessibleElement(host, @"Checks 0");
  XCTAssertNotNil(status);
  XCTAssertEqual([status accessibilityTraits] & UIAccessibilityTraitUpdatesFrequently, 0UL);

  id header = FindAccessibleElement(host, ^BOOL(id element) {
    return [[element accessibilityLabel] isEqualToString:@"Advanced options"] &&
           ([element accessibilityTraits] & UIAccessibilityTraitButton) != 0;
  });
  UIAccessibilityCustomAction *expand = ActionNamed(header, @"Expand");
  XCTAssertNotNil(expand);
  XCTAssertTrue(Invoke(expand));
  if (expand == nil) {
    [host accessibilityPerformEscape];
    return;
  }
  [self waitForLabel:@"Network details" inHost:host];
  XCTAssertEqualObjects([header accessibilityValue], @"Expanded");
  XCTAssertFalse(Invoke(expand));
  XCTAssertTrue([AccessibleElement(host, @"Network details") accessibilityActivate]);
  [self waitForLabel:@"Checks 1" inHost:host];
  XCTAssertEqual(status, AccessibleElement(host, @"Checks 1"));
  XCTAssertNotEqual([status accessibilityTraits] & UIAccessibilityTraitUpdatesFrequently, 0UL);
  XCTAssertTrue(Invoke(ActionNamed(header, @"Collapse")));
  NSPredicate *collapsed = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Network details") == nil &&
           [[header accessibilityValue] isEqualToString:@"Collapsed"];
  }];
  [self expectationForPredicate:collapsed evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  id group = AccessibleElement(host, @"Optional settings");
  XCTAssertTrue([group accessibilityActivate]);
  NSPredicate *disabled = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    id child = AccessibleElement(view, @"Test settings");
    return child != nil && ([child accessibilityTraits] & UIAccessibilityTraitNotEnabled) != 0;
  }];
  [self expectationForPredicate:disabled evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertFalse([AccessibleElement(host, @"Test settings") accessibilityActivate]);
  XCTAssertTrue([group accessibilityActivate]);
  NSPredicate *enabled = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    id child = AccessibleElement(view, @"Test settings");
    return child != nil && ([child accessibilityTraits] & UIAccessibilityTraitNotEnabled) == 0;
  }];
  [self expectationForPredicate:enabled evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue([AccessibleElement(host, @"Test settings") accessibilityActivate]);
  [self waitForLabel:@"Checks 2" inHost:host];
  XCTAssertEqual(status, AccessibleElement(host, @"Checks 2"));
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitDismissesTheModalDrawerWithItsControlsAvailable {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Open tools drawer" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Open tools drawer") accessibilityActivate]);
  [self waitForLabel:@"Check instrument" inHost:host];
  id title = FindAccessibleElement(host, ^BOOL(id element) {
    return [[element accessibilityLabel] isEqualToString:@"Inspection tools"] &&
           ([element accessibilityTraits] & UIAccessibilityTraitHeader) != 0;
  });
  XCTAssertNotNil(title);
  XCTAssertNil(AccessibleElement(host, @"Review options"));
  XCTAssertTrue([AccessibleElement(host, @"Check instrument") accessibilityActivate]);
  [self waitForLabel:@"Tool runs 1" inHost:host];
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Tool closures 1" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Check instrument"));
  XCTAssertFalse([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

@end
