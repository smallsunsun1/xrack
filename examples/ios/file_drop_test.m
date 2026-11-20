#import <XCTest/XCTest.h>

#include "drop_test_support.h"

static void RegisterPromise(NSItemProvider *provider, NSString *type, NSURL *source,
                            NSItemProviderFileOptions options, NSTimeInterval delay) {
  [provider registerFileRepresentationForTypeIdentifier:type
                                            fileOptions:options
                                             visibility:NSItemProviderRepresentationVisibilityAll
                                            loadHandler:^NSProgress *(void (^complete)(NSURL *, BOOL, NSError *)) {
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(delay * NSEC_PER_SEC)),
                   dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
      NSError *error = source == nil ? [NSError errorWithDomain:NSCocoaErrorDomain
                                                         code:NSFileReadNoSuchFileError
                                                     userInfo:nil] : nil;
      complete(source, NO, error);
    });
    return nil;
  }];
}

static NSItemProvider *FilePromise(NSURL *source, NSString *name, NSTimeInterval delay) {
  NSItemProvider *provider = [[NSItemProvider alloc] init];
  provider.suggestedName = name;
  RegisterPromise(provider, @"public.plain-text", source, 0, delay);
  return provider;
}

@interface XRackFileDropTest : XCTestCase
@end

@implementation XRackFileDropTest

- (void)testDemoDeclaresAllIpadOrientationsForSplitView {
  NSURL *plist = [NSBundle.mainBundle.bundleURL URLByAppendingPathComponent:@"Info.plist"];
  NSDictionary *info = [NSDictionary dictionaryWithContentsOfURL:plist error:NULL];
  XCTAssertNotNil(info);
  NSArray<NSString *> *orientations = info[@"UISupportedInterfaceOrientations~ipad"];
  XCTAssertEqualObjects([NSSet setWithArray:orientations ?: @[]],
                        ([NSSet setWithArray:@[@"UIInterfaceOrientationPortrait",
                                               @"UIInterfaceOrientationPortraitUpsideDown",
                                               @"UIInterfaceOrientationLandscapeLeft",
                                               @"UIInterfaceOrientationLandscapeRight"]]));
  XCTAssertFalse([info[@"UIRequiresFullScreen"] boolValue]);
}

- (void)testUIKitCopiesOrderedFilesAndRejectsPartialOrMismatchedBatches {
  self.continueAfterFailure = NO;
  NSPredicate *ready = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    UIView *host = WindowAccessibilityHost();
    return AccessibleElement(host, @"TXT / Markdown") != nil && DropInteraction(host) != nil;
  }];
  [self expectationForPredicate:ready evaluatedWithObject:self handler:nil];
  [self waitForExpectationsWithTimeout:20 handler:nil];

  UIView *host = WindowAccessibilityHost();
  UIDropInteraction *interaction = DropInteraction(host);
  id<UIDropInteractionDelegate> delegate = interaction.delegate;
  CGRect frame = [AccessibleElement(host, @"TXT / Markdown") accessibilityFrame];
  CGPoint point = CGPointMake(CGRectGetMidX(frame), CGRectGetMidY(frame));
  NSURL *directory = [[NSURL fileURLWithPath:NSTemporaryDirectory() isDirectory:YES]
      URLByAppendingPathComponent:NSUUID.UUID.UUIDString isDirectory:YES];
  NSFileManager *files = NSFileManager.defaultManager;
  XCTAssertTrue([files createDirectoryAtURL:directory
                withIntermediateDirectories:YES attributes:nil error:NULL]);
  [self addTeardownBlock:^{ [files removeItemAtURL:directory error:NULL]; }];
  NSURL *first = [directory URLByAppendingPathComponent:@"巡检.TXT"];
  NSURL *second = [directory URLByAppendingPathComponent:@"second.md"];
  NSURL *original = [directory URLByAppendingPathComponent:@"original.md"];
  NSURL *metadata = [directory URLByAppendingPathComponent:@"metadata"];
  XCTAssertTrue([@"first file" writeToURL:first atomically:YES
                               encoding:NSUTF8StringEncoding error:NULL]);
  XCTAssertTrue([@"second file" writeToURL:second atomically:YES
                                encoding:NSUTF8StringEncoding error:NULL]);
  XCTAssertTrue([@"original document" writeToURL:original atomically:YES
                                       encoding:NSUTF8StringEncoding error:NULL]);
  XCTAssertTrue([@"metadata only" writeToURL:metadata atomically:YES
                                   encoding:NSUTF8StringEncoding error:NULL]);

  NSItemProvider *secondProvider = [[NSItemProvider alloc] initWithItem:second
                                                       typeIdentifier:@"public.file-url"];
  NSItemProvider *document = [[NSItemProvider alloc] init];
  document.suggestedName = @"original.md";
  RegisterPromise(document, @"public.item", metadata, 0, 0);
  RegisterPromise(document, @"public.plain-text", original,
                  NSItemProviderFileOptionOpenInPlace, 0.15);
  id<UIDropSession> valid = Session(@[FilePromise(first, @"巡检.TXT", 0.35),
                                      secondProvider, document], point);
  XCTAssertTrue([delegate dropInteraction:interaction canHandleSession:valid]);
  XCTAssertEqual([delegate dropInteraction:interaction sessionDidUpdate:valid].operation,
                 UIDropOperationCopy);
  XCTAssertNotNil(AccessibleElement(host, @"尚未接收"));
  [delegate dropInteraction:interaction performDrop:valid];
  [delegate dropInteraction:interaction sessionDidEnd:valid];

  NSPredicate *delivered = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"文档 · 已接收 1 批") != nil;
  }];
  [self expectationForPredicate:delivered evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:10 handler:nil];
  NSMutableArray<NSString *> *paths = [NSMutableArray array];
  FindAccessibleElement(host, ^BOOL(id element) {
    NSString *label = [element accessibilityLabel];
    if ([label containsString:@"/xrack-drop-"] &&
        ([label hasSuffix:@"/巡检.TXT"] || [label hasSuffix:@"/second.md"] ||
         [label hasSuffix:@"/original.md"])) {
      [paths addObject:label];
    }
    return NO;
  });
  XCTAssertEqualObjects([paths valueForKey:@"lastPathComponent"],
                        (@[@"巡检.TXT", @"second.md", @"original.md"]));
  XCTAssertEqualObjects([NSString stringWithContentsOfFile:paths[0]
                                                  encoding:NSUTF8StringEncoding error:NULL], @"first file");
  XCTAssertEqualObjects([NSString stringWithContentsOfFile:paths[1]
                                                  encoding:NSUTF8StringEncoding error:NULL], @"second file");
  XCTAssertEqualObjects([NSString stringWithContentsOfFile:paths[2]
                                                  encoding:NSUTF8StringEncoding error:NULL], @"original document");

  NSItemProvider *metadataOnly = [[NSItemProvider alloc] init];
  metadataOnly.suggestedName = @"metadata.md";
  RegisterPromise(metadataOnly, @"public.item", metadata, 0, 0);
  XCTAssertFalse([delegate dropInteraction:interaction
                           canHandleSession:Session(@[metadataOnly], point)]);

  id<UIDropSession> wrong = Session(@[FilePromise(first, @"wrong.png", 0)], point);
  XCTAssertEqual([delegate dropInteraction:interaction sessionDidUpdate:wrong].operation,
                 UIDropOperationCancel);
  [delegate dropInteraction:interaction sessionDidEnd:wrong];

  id<UIDropSession> partial = Session(@[FilePromise(first, @"first.txt", 0),
                                      FilePromise(nil, @"missing.txt", 0.15)], point);
  XCTAssertEqual([delegate dropInteraction:interaction sessionDidUpdate:partial].operation,
                 UIDropOperationCopy);
  [delegate dropInteraction:interaction performDrop:partial];
  [delegate dropInteraction:interaction sessionDidEnd:partial];
  NSString *message = @"接收失败 · Could not read one or more dropped files.";
  NSPredicate *failed = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, message) != nil;
  }];
  [self expectationForPredicate:failed evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertNotNil(AccessibleElement(host, @"文档 · 已接收 1 批"));
  XCTAssertNotNil(AccessibleElement(host, paths[0]));
  XCTAssertNil(FindAccessibleElement(host, ^BOOL(id element) {
    return [[element accessibilityLabel] hasSuffix:@"/first.txt"];
  }));

  id<UIDropSession> retry = Session(@[FilePromise(first, @"retry.txt", 0)], point);
  XCTAssertEqual([delegate dropInteraction:interaction sessionDidUpdate:retry].operation,
                 UIDropOperationCopy);
  [delegate dropInteraction:interaction performDrop:retry];
  [delegate dropInteraction:interaction sessionDidEnd:retry];
  NSPredicate *recovered = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"文档 · 已接收 2 批") != nil &&
           AccessibleElement(view, message) == nil;
  }];
  [self expectationForPredicate:recovered evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

@end
