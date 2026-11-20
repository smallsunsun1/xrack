package dev.xrack.share_test;

import android.app.Activity;
import android.os.Bundle;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

public final class ShareReceiverActivity extends Activity {
    @Override
    public void onCreate(Bundle state) {
        super.onCreate(state);
        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(24, 72, 24, 24);
        TextView heading = new TextView(this);
        heading.setText("Received note");
        heading.setTextSize(24);
        content.addView(heading);
        TextView type = new TextView(this);
        type.setText(getIntent().getType());
        content.addView(type);
        TextView note = new TextView(this);
        note.setText(getIntent().getCharSequenceExtra(android.content.Intent.EXTRA_TEXT));
        note.setTextSize(20);
        content.addView(note);
        Button close = new Button(this);
        close.setText("Return to Pocket Tasks");
        close.setOnClickListener(view -> finish());
        content.addView(close);
        setContentView(content);
    }
}
