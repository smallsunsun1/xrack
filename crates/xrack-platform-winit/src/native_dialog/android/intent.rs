use jni::bind_java_type;

bind_java_type! {
    pub(super) Context => android.content.Context,
    methods { fn get_package_name() -> JString }
}

bind_java_type! { pub(super) IntentSender => android.content.IntentSender }

bind_java_type! {
    pub(super) Intent => android.content.Intent,
    type_map {
        Context => android.content.Context,
        IntentSender => android.content.IntentSender,
    },
    constructors { fn new(action: JString) },
    methods {
        fn add_flags(flags: jint) -> Intent,
        fn add_category(category: JString) -> Intent,
        fn set_type(mime: JString) -> Intent,
        fn set_class_name(context: Context, name: JString) -> Intent,
        fn get_action() -> JString,
        fn has_extra(key: JString) -> bool,
        fn put_boolean {
            name = "putExtra",
            sig = (key: JString, value: bool) -> Intent,
        },
        fn put_string {
            name = "putExtra",
            sig = (key: JString, value: JString) -> Intent,
        },
        fn put_strings {
            name = "putExtra",
            sig = (key: JString, value: JString[]) -> Intent,
        },
        static fn create_chooser(target: Intent, title: JCharSequence, callback: IntentSender) -> Intent,
    }
}
