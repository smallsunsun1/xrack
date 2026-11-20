#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

static UIView<UITextInput> *ActiveTextInput(UIView *view) {
  if (view.isFirstResponder && [view conformsToProtocol:@protocol(UITextInput)]) {
    return (UIView<UITextInput> *)view;
  }
  for (UIView *child in view.subviews) {
    UIView<UITextInput> *input = ActiveTextInput(child);
    if (input != nil) {
      return input;
    }
  }
  return nil;
}

static BOOL ChooseRange(id<UITextInput> input, NSInteger start, NSInteger end) {
  UITextPosition *origin = input.beginningOfDocument;
  UITextPosition *anchor = [input positionFromPosition:origin offset:start];
  UITextPosition *focus = [input positionFromPosition:origin offset:end];
  if (anchor == nil || focus == nil) {
    return NO;
  }
  input.selectedTextRange = [input textRangeFromPosition:anchor toPosition:focus];
  return input.selectedTextRange != nil;
}

static BOOL WaitForCondition(BOOL (^condition)(void)) {
  NSPredicate *predicate = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return condition();
  }];
  XCTNSPredicateExpectation *expectation =
      [[XCTNSPredicateExpectation alloc] initWithPredicate:predicate object:nil];
  return [XCTWaiter waitForExpectations:@[expectation] timeout:8] == XCTWaiterResultCompleted;
}

#define Await(...) XCTAssertTrue(WaitForCondition((__VA_ARGS__)), @"Timed out waiting for %s", #__VA_ARGS__)

@interface XRackTextSelectionTest : XCTestCase
@end

@implementation XRackTextSelectionTest

- (void)confirmMarkedTextInHost:(UIView *)host
                          name:(NSString *)name
                     responder:(Class)responder
                        offset:(NSInteger)offset
                        prefix:(NSString *)prefix {
  XCTAssertTrue([AccessibleElement(host, [@"Focus " stringByAppendingString:name.lowercaseString])
      accessibilityActivate]);
  Await(^BOOL { return [ActiveTextInput(host.window) isKindOfClass:responder]; });
  UIView<UITextInput> *input = ActiveTextInput(host.window);
  NSString *editor = [@"Selection " stringByAppendingString:name.lowercaseString];
  NSString *editsBefore = [name stringByAppendingString:@" edits: 1"];
  NSString *editsAfter = [name stringByAppendingString:@" edits: 2"];
  NSString *copyBlocked = [name stringByAppendingString:@" copy blocked"];
  NSString *selectedOriginal = [NSString stringWithFormat:@"%@ selection: X [%ld, %ld)",
                                name, (long)offset, (long)(offset + 1)];
  // UIKit may update its selection and mark the range before XRack's next event.
  XCTAssertTrue(ChooseRange(input, offset, offset + 1));
  [input setMarkedText:@"shi" selectedRange:NSMakeRange(3, 0)];
  XCTAssertNotNil(input.markedTextRange);
  BOOL reachedMarkedSelection = WaitForCondition(^BOOL {
    return AccessibleElement(host, selectedOriginal) != nil &&
           AccessibleElement(host, copyBlocked) != nil;
  });
  if (!reachedMarkedSelection) {
    NSString *selectionPrefix = [name stringByAppendingString:@" selection:"];
    id selection = FindAccessibleElement(host, ^BOOL(id element) {
      return [[element accessibilityLabel] hasPrefix:selectionPrefix];
    });
    NSString *native = [input textInRange:[input textRangeFromPosition:input.beginningOfDocument
                                                           toPosition:input.endOfDocument]];
    XCTFail(@"Expected %@ while marked. Got %@, UIKit text %@, copy blocked %d",
            selectedOriginal, [selection accessibilityLabel], native,
            AccessibleElement(host, copyBlocked) != nil);
  }
  XCTAssertNotNil(AccessibleElement(host, editsBefore));
  XCTAssertEqualObjects([AccessibleElement(host, editor) accessibilityValue],
                        [prefix stringByAppendingString:@"AXB"]);

  [input setMarkedText:@"試験" selectedRange:NSMakeRange(2, 0)];
  XCTAssertNotNil(input.markedTextRange);
  [input unmarkText];
  XCTAssertNil(input.markedTextRange);
  Await(^BOOL { return AccessibleElement(host, editsAfter) != nil; });
  XCTAssertEqualObjects([AccessibleElement(host, editor) accessibilityValue],
                        [prefix stringByAppendingString:@"A試験B"]);
}

- (void)replaceModelTextInHost:(UIView *)host
                          name:(NSString *)name
                     responder:(Class)responder
                        offset:(NSInteger)offset
                 queueCandidate:(BOOL)queueCandidate {
  XCTAssertTrue([AccessibleElement(host, [@"Focus " stringByAppendingString:name.lowercaseString])
      accessibilityActivate]);
  Await(^BOOL { return [ActiveTextInput(host.window) isKindOfClass:responder]; });
  UIView<UITextInput> *input = ActiveTextInput(host.window);
  NSString *editor = [@"Selection " stringByAppendingString:name.lowercaseString];
  NSString *updated = [@"Updated " stringByAppendingString:name.lowercaseString];
  NSString *copyAvailable = [name stringByAppendingString:@" copy available"];
  NSString *copyBlocked = [name stringByAppendingString:@" copy blocked"];
  XCTAssertTrue(ChooseRange(input, offset, offset + 1));
  Await(^BOOL { return AccessibleElement(host, copyAvailable) != nil; });
  [input setMarkedText:@"stale" selectedRange:NSMakeRange(5, 0)];
  XCTAssertNotNil(input.markedTextRange);
  Await(^BOOL { return AccessibleElement(host, copyBlocked) != nil; });

  XCTAssertTrue([AccessibleElement(host, [@"Replace " stringByAppendingString:name.lowercaseString])
      accessibilityActivate]);
  if (queueCandidate) {
    [input unmarkText];
  }
  NSString *(^nativeValue)(void) = ^NSString * {
    return [input textInRange:[input textRangeFromPosition:input.beginningOfDocument
                                                toPosition:input.endOfDocument]];
  };
  XCTAssertTrue(WaitForCondition(^BOOL {
    return [[AccessibleElement(host, editor) accessibilityValue] isEqualToString:updated] &&
           input.markedTextRange == nil && [nativeValue() isEqualToString:updated];
  }), @"Expected %@; model %@; UIKit %@; marked %@; focused %@",
      updated, [AccessibleElement(host, editor) accessibilityValue], nativeValue(),
      input.markedTextRange, ActiveTextInput(host.window));
  XCTAssertEqual(ActiveTextInput(host.window), input);
  XCTAssertNotNil(AccessibleElement(host, [name stringByAppendingString:@" edits: 2"]));
  NSUInteger length = updated.length;
  XCTAssertTrue(ChooseRange(input, length, length));
  [input insertText:@"!"];
  Await(^BOOL {
    return [[AccessibleElement(host, editor) accessibilityValue]
        isEqualToString:[updated stringByAppendingString:@"!"]];
  });
  XCTAssertNotNil(AccessibleElement(host, [name stringByAppendingString:@" edits: 3"]));
}

- (void)discardQueuedCandidateWhenSwitchingFieldsInHost:(UIView *)host {
  UIView<UITextInput> *notes = ActiveTextInput(host.window);
  XCTAssertTrue([notes isKindOfClass:UITextView.class]);
  XCTAssertTrue(ChooseRange(notes, 0, 7));
  Await(^BOOL { return AccessibleElement(host, @"Notes copy available") != nil; });
  [notes setMarkedText:@"other" selectedRange:NSMakeRange(5, 0)];
  XCTAssertNotNil(notes.markedTextRange);
  Await(^BOOL { return AccessibleElement(host, @"Notes copy blocked") != nil; });
  XCTAssertTrue([AccessibleElement(host, @"Focus line") accessibilityActivate]);
  [notes unmarkText];
  Await(^BOOL {
    return [ActiveTextInput(host.window) isKindOfClass:UITextField.class];
  });
  XCTAssertEqualObjects([AccessibleElement(host, @"Selection notes") accessibilityValue],
                        @"Updated notes!");
  XCTAssertEqualObjects([AccessibleElement(host, @"Selection line") accessibilityValue],
                        @"Updated line!");
  XCTAssertNotNil(AccessibleElement(host, @"Notes edits: 3"));
  XCTAssertNotNil(AccessibleElement(host, @"Line edits: 3"));
}

- (void)verifyUIKitUndoManagerInHost:(UIView *)host {
  UIView<UITextInput> *line = ActiveTextInput(host.window);
  XCTAssertTrue([line isKindOfClass:UITextField.class]);
  NSUndoManager *lineHistory = line.undoManager;
  XCTAssertNotNil(lineHistory);
  XCTAssertFalse(lineHistory.undoRegistrationEnabled);
  XCTAssertTrue(lineHistory.canUndo);
  XCTAssertFalse(lineHistory.canRedo);
  [lineHistory undo];
  Await(^BOOL {
    return [[AccessibleElement(host, @"Selection line") accessibilityValue]
        isEqualToString:@"Updated line"] && lineHistory.canRedo;
  });
  XCTAssertFalse(lineHistory.canUndo);
  [lineHistory redo];
  Await(^BOOL {
    return [[AccessibleElement(host, @"Selection line") accessibilityValue]
        isEqualToString:@"Updated line!"] && lineHistory.canUndo;
  });

  XCTAssertTrue([AccessibleElement(host, @"Focus notes") accessibilityActivate]);
  Await(^BOOL { return [ActiveTextInput(host.window) isKindOfClass:UITextView.class]; });
  UIView<UITextInput> *notes = ActiveTextInput(host.window);
  NSUndoManager *notesHistory = notes.undoManager;
  XCTAssertNotNil(notesHistory);
  XCTAssertNotEqual(lineHistory, notesHistory);
  XCTAssertNil(line.undoManager);
  XCTAssertFalse(notesHistory.undoRegistrationEnabled);
  XCTAssertFalse(lineHistory.canUndo);
  XCTAssertFalse(lineHistory.canRedo);
  [lineHistory undo];
  XCTAssertTrue(notesHistory.canUndo);
  [notesHistory undo];
  Await(^BOOL {
    return [[AccessibleElement(host, @"Selection notes") accessibilityValue]
        isEqualToString:@"Updated notes"] && notesHistory.canRedo;
  });
  XCTAssertEqualObjects([AccessibleElement(host, @"Selection line") accessibilityValue],
                        @"Updated line!");
  [notesHistory redo];
  Await(^BOOL {
    return [[AccessibleElement(host, @"Selection notes") accessibilityValue]
        isEqualToString:@"Updated notes!"] && notesHistory.canUndo;
  });

  [notes setMarkedText:@"candidate" selectedRange:NSMakeRange(9, 0)];
  XCTAssertNotNil(notes.markedTextRange);
  XCTAssertFalse(notesHistory.canUndo);
  XCTAssertFalse(notesHistory.canRedo);
}

- (void)verifyUIKitTextCommandsInHost:(UIView *)host {
  UIView<UITextInput> *notes = ActiveTextInput(host.window);
  XCTAssertTrue([notes isKindOfClass:UITextView.class]);
  XCTAssertNotNil(notes.markedTextRange);
  UIPasteboard *pasteboard = UIPasteboard.generalPasteboard;
  pasteboard.string = @"clipboard seed";
  XCTAssertFalse([notes canPerformAction:@selector(paste:) withSender:nil]);
  [notes paste:nil];
  XCTAssertNotNil(notes.markedTextRange);
  XCTAssertEqualObjects([AccessibleElement(host, @"Selection notes") accessibilityValue],
                        @"Updated notes!");

  XCTAssertTrue([AccessibleElement(host, @"Focus line") accessibilityActivate]);
  Await(^BOOL { return [ActiveTextInput(host.window) isKindOfClass:UITextField.class]; });
  XCTAssertFalse([notes canPerformAction:@selector(paste:) withSender:nil]);
  UIView<UITextInput> *line = ActiveTextInput(host.window);
  XCTAssertFalse([line canPerformAction:@selector(copy:) withSender:nil]);
  XCTAssertTrue([line canPerformAction:@selector(selectAll:) withSender:nil]);
  [line selectAll:nil];
  Await(^BOOL { return AccessibleElement(host, @"Line selection: Updated line! [0, 13)") != nil; });
  XCTAssertTrue([line canPerformAction:@selector(copy:) withSender:nil]);
  XCTAssertTrue([line canPerformAction:@selector(cut:) withSender:nil]);

  [line copy:nil];
  Await(^BOOL { return [pasteboard.string isEqualToString:@"Updated line!"]; });
  XCTAssertEqualObjects([AccessibleElement(host, @"Selection line") accessibilityValue],
                        @"Updated line!");
  [line cut:nil];
  Await(^BOOL {
    return [[AccessibleElement(host, @"Selection line") accessibilityValue] length] == 0;
  });
  XCTAssertTrue([line canPerformAction:@selector(paste:) withSender:nil]);
  [line paste:nil];
  Await(^BOOL {
    return [[AccessibleElement(host, @"Selection line") accessibilityValue]
        isEqualToString:@"Updated line!"];
  });
  [line.undoManager undo];
  Await(^BOOL {
    return [[AccessibleElement(host, @"Selection line") accessibilityValue] length] == 0;
  });
  [line.undoManager redo];
  Await(^BOOL {
    return [[AccessibleElement(host, @"Selection line") accessibilityValue]
        isEqualToString:@"Updated line!"];
  });

  XCTAssertTrue([AccessibleElement(host, @"Focus notes") accessibilityActivate]);
  Await(^BOOL { return [ActiveTextInput(host.window) isKindOfClass:UITextView.class]; });
  notes = ActiveTextInput(host.window);
  XCTAssertTrue(ChooseRange(notes, 0, 0));
  Await(^BOOL { return AccessibleElement(host, @"Notes selection:  [0, 0)") != nil; });
  XCTAssertTrue([notes canPerformAction:@selector(select:) withSender:nil]);
  [notes select:nil];
  Await(^BOOL { return AccessibleElement(host, @"Notes selection: Updated [0, 7)") != nil; });
  pasteboard.string = @"First\nSecond";
  [notes paste:nil];
  Await(^BOOL {
    return [[AccessibleElement(host, @"Selection notes") accessibilityValue]
        isEqualToString:@"First\nSecond notes!"];
  });
  pasteboard.items = @[];
  XCTAssertFalse([notes canPerformAction:@selector(paste:) withSender:nil]);
}

- (void)testUIKitSelectionReachesLineAndNotesAndValidatesMarkedTextLifetime {
  self.continueAfterFailure = NO;
  Await(^BOOL {
    return AccessibleElement(WindowAccessibilityHost(), @"Focus line") != nil;
  });
  UIView *host = WindowAccessibilityHost();
  XCTAssertTrue([AccessibleElement(host, @"Focus line") accessibilityActivate]);
  Await(^BOOL { return [ActiveTextInput(host.window) isKindOfClass:UITextField.class]; });
  UIView<UITextInput> *line = ActiveTextInput(host.window);
  XCTAssertTrue(ChooseRange(line, 1, 3));
  Await(^BOOL { return AccessibleElement(host, @"Line selection: 🧪 [1, 5)") != nil; });
  XCTAssertNotNil(AccessibleElement(host, @"Line edits: 0"));
  XCTAssertEqualObjects([AccessibleElement(host, @"Selection line") accessibilityValue], @"A🧪B");
  [line insertText:@"X"];
  Await(^BOOL { return AccessibleElement(host, @"Line edits: 1") != nil; });
  XCTAssertEqualObjects([AccessibleElement(host, @"Selection line") accessibilityValue], @"AXB");

  XCTAssertTrue([AccessibleElement(host, @"Focus notes") accessibilityActivate]);
  Await(^BOOL { return [ActiveTextInput(host.window) isKindOfClass:UITextView.class]; });
  UIView<UITextInput> *notes = ActiveTextInput(host.window);
  XCTAssertTrue(ChooseRange(notes, 7, 9));
  Await(^BOOL { return AccessibleElement(host, @"Notes selection: 🧪 [7, 11)") != nil; });
  XCTAssertNotNil(AccessibleElement(host, @"Notes edits: 0"));
  XCTAssertEqualObjects([AccessibleElement(host, @"Selection notes") accessibilityValue], @"First\nA🧪B");
  [notes insertText:@"X"];
  Await(^BOOL { return AccessibleElement(host, @"Notes edits: 1") != nil; });
  XCTAssertEqualObjects([AccessibleElement(host, @"Selection notes") accessibilityValue], @"First\nAXB");

  [self confirmMarkedTextInHost:host name:@"Line" responder:UITextField.class offset:1 prefix:@""];
  [self confirmMarkedTextInHost:host name:@"Notes" responder:UITextView.class offset:7 prefix:@"First\n"];
  [self replaceModelTextInHost:host name:@"Line" responder:UITextField.class offset:0 queueCandidate:NO];
  [self replaceModelTextInHost:host name:@"Notes" responder:UITextView.class offset:6 queueCandidate:YES];
  [self discardQueuedCandidateWhenSwitchingFieldsInHost:host];
  [self verifyUIKitUndoManagerInHost:host];
  [self verifyUIKitTextCommandsInHost:host];

  notes = ActiveTextInput(host.window);
  XCTAssertTrue([notes isKindOfClass:UITextView.class]);
  XCTAssertTrue([AccessibleElement(host, @"Replace notes") accessibilityActivate]);
  XCTAssertTrue([notes resignFirstResponder]);
  Await(^BOOL {
    return ActiveTextInput(host.window) == nil &&
        [[AccessibleElement(host, @"Selection notes") accessibilityValue]
            isEqualToString:@"Updated notes"];
  });
}

@end
