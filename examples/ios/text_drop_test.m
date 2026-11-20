#import <XCTest/XCTest.h>

#include "drop_test_support.h"

static NSItemProvider *TextProvider(NSString *text, NSTimeInterval delay) {
  NSItemProvider *provider = [[NSItemProvider alloc] init];
  provider.preferredPresentationStyle = UIPreferredPresentationStyleInline;
  [provider registerDataRepresentationForTypeIdentifier:@"public.utf8-plain-text"
                                             visibility:NSItemProviderRepresentationVisibilityAll
                                            loadHandler:^NSProgress *(void (^complete)(NSData *, NSError *)) {
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(delay * NSEC_PER_SEC)),
                   dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
      if (text != nil) {
        complete([text dataUsingEncoding:NSUTF8StringEncoding], nil);
      } else {
        complete(nil, [NSError errorWithDomain:NSCocoaErrorDomain
                                         code:NSFileReadUnknownError userInfo:nil]);
      }
    });
    return nil;
  }];
  return provider;
}

static CGPoint TextPoint(UIView *host, NSString *label) {
  CGRect frame = [AccessibleElement(host, label) accessibilityFrame];
  return CGPointMake(CGRectGetMinX(frame) + 4, CGRectGetMidY(frame));
}

@interface XRackTextDropTest : XCTestCase
@end

@implementation XRackTextDropTest

- (void)expectValue:(NSString *)value forLabel:(NSString *)label inHost:(UIView *)host {
  NSPredicate *matches = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return [[AccessibleElement(view, label) accessibilityValue] isEqual:value];
  }];
  XCTNSPredicateExpectation *expectation =
      [[XCTNSPredicateExpectation alloc] initWithPredicate:matches object:host];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[expectation] timeout:5], XCTWaiterResultCompleted,
                 @"%@ expected '%@', got '%@'", label, value,
                 [AccessibleElement(host, label) accessibilityValue]);
}

- (void)testUIKitRoutesInlineTextAndCommitsCompleteCurrentBatches {
  self.continueAfterFailure = NO;
  NSPredicate *ready = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    UIView *host = WindowAccessibilityHost();
    return AccessibleElement(host, @"Drop title") != nil && DropInteraction(host) != nil;
  }];
  [self expectationForPredicate:ready evaluatedWithObject:self handler:nil];
  [self waitForExpectationsWithTimeout:20 handler:nil];

  UIView *host = WindowAccessibilityHost();
  UIDropInteraction *interaction = DropInteraction(host);
  id<UIDropInteractionDelegate> delegate = interaction.delegate;
  NSItemProvider *inlineText = [[NSItemProvider alloc] initWithObject:@"巡检🧭"];
  for (NSString *label in @[@"Read-only drop target", @"Disabled drop target"]) {
    XCTAssertNotNil(AccessibleElement(host, label));
    id<UIDropSession> blocked = Session(@[inlineText], TextPoint(host, label));
    XCTAssertEqual([delegate dropInteraction:interaction sessionDidUpdate:blocked].operation,
                   UIDropOperationCancel);
    [delegate dropInteraction:interaction sessionDidEnd:blocked];
  }
  NSItemProvider *attachment = TextProvider(@"Contents", 0);
  attachment.preferredPresentationStyle = UIPreferredPresentationStyleAttachment;
  attachment.suggestedName = @"notes.txt";
  id<UIDropSession> asFile = Session(@[attachment], TextPoint(host, @"Drop title"));
  XCTAssertEqual([delegate dropInteraction:interaction sessionDidUpdate:asFile].operation,
                 UIDropOperationCancel);
  [delegate dropInteraction:interaction sessionDidEnd:asFile];

  id<UIDropSession> title = Session(@[inlineText], TextPoint(host, @"Drop title"));
  XCTAssertTrue([delegate dropInteraction:interaction canHandleSession:title]);
  XCTAssertEqual([delegate dropInteraction:interaction sessionDidUpdate:title].operation,
                 UIDropOperationCopy);
  [delegate dropInteraction:interaction performDrop:title];
  [delegate dropInteraction:interaction sessionDidEnd:title];
  [self expectValue:@"巡检🧭A🧪B" forLabel:@"Drop title" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Drop edits 1"));
  XCTAssertTrue([AccessibleElement(host, @"Undo title") accessibilityActivate]);
  [self expectValue:@"A🧪B" forLabel:@"Drop title" inHost:host];

  NSItemProvider *namedInlineText = TextProvider(@"First", 0.30);
  namedInlineText.suggestedName = @"selection.txt";
  id<UIDropSession> notes = Session(@[namedInlineText,
                                     TextProvider(@"Second 🧭", 0)], TextPoint(host, @"Drop notes"));
  XCTAssertEqual([delegate dropInteraction:interaction sessionDidUpdate:notes].operation,
                 UIDropOperationCopy);
  [delegate dropInteraction:interaction performDrop:notes];
  [delegate dropInteraction:interaction sessionDidEnd:notes];
  [self expectValue:@"First\nSecond 🧭" forLabel:@"Drop notes" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Drop edits 3"));

  id<UIDropSession> failed = Session(@[TextProvider(@"Partial", 0), TextProvider(nil, 0.15)],
                                     TextPoint(host, @"Drop notes"));
  XCTAssertEqual([delegate dropInteraction:interaction sessionDidUpdate:failed].operation,
                 UIDropOperationCopy);
  [delegate dropInteraction:interaction performDrop:failed];
  [delegate dropInteraction:interaction sessionDidEnd:failed];
  NSPredicate *changedNotes = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return ![[AccessibleElement(view, @"Drop notes") accessibilityValue] isEqual:@"First\nSecond 🧭"];
  }];
  XCTestExpectation *noPartialEdit =
      [self expectationForPredicate:changedNotes evaluatedWithObject:host handler:nil];
  noPartialEdit.inverted = YES;
  [self waitForExpectationsWithTimeout:0.6 handler:nil];

  id<UIDropSession> stale = Session(@[TextProvider(@"Stale", 0.8)], TextPoint(host, @"Drop title"));
  XCTAssertEqual([delegate dropInteraction:interaction sessionDidUpdate:stale].operation,
                 UIDropOperationCopy);
  [delegate dropInteraction:interaction performDrop:stale];
  [delegate dropInteraction:interaction sessionDidEnd:stale];
  XCTestExpectation *changed = [self expectationWithDescription:@"Change title while provider loads"];
  dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(0.15 * NSEC_PER_SEC)),
                 dispatch_get_main_queue(), ^{
    XCTAssertTrue([AccessibleElement(host, @"Change title") accessibilityActivate]);
    [changed fulfill];
  });
  [self waitForExpectationsWithTimeout:3 handler:nil];
  [self expectValue:@"Updated" forLabel:@"Drop title" inHost:host];
  NSPredicate *unexpected = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return ![[AccessibleElement(view, @"Drop title") accessibilityValue] isEqual:@"Updated"] ||
           ![[AccessibleElement(view, @"Drop notes") accessibilityValue] isEqual:@"First\nSecond 🧭"];
  }];
  XCTestExpectation *unchanged = [self expectationForPredicate:unexpected evaluatedWithObject:host handler:nil];
  unchanged.inverted = YES;
  [self waitForExpectationsWithTimeout:1.5 handler:nil];
  XCTAssertNotNil(AccessibleElement(host, @"Drop edits 3"));
}

@end
