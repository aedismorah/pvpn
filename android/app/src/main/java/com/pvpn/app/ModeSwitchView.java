package com.pvpn.app;

import android.animation.ValueAnimator;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.util.AttributeSet;
import android.view.MotionEvent;
import android.view.View;

/** Full-width, squarish White list / Black list switch, matching the desktop UI. */
public class ModeSwitchView extends View {
    private static final int TRACK = 0xFF081116;
    private static final int BORDER = 0xFF22343A;
    private static final int ACTIVE = 0xFF14564C;
    private static final int TEXT = 0xFFE8FFF8;
    private static final int MUTED = 0xFF8BA29E;

    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final RectF track = new RectF();
    private final RectF active = new RectF();
    private final float density;
    private boolean checked;
    private float anim;
    private ValueAnimator animator;

    public interface OnModeChangedListener {
        void onModeChanged(boolean blackList);
    }

    private OnModeChangedListener listener;

    public ModeSwitchView(Context context) {
        this(context, null);
    }

    public ModeSwitchView(Context context, AttributeSet attrs) {
        super(context, attrs);
        density = getResources().getDisplayMetrics().density;
        setClickable(true);
    }

    public void setOnModeChangedListener(OnModeChangedListener listener) {
        this.listener = listener;
    }

    public void setChecked(boolean checked, boolean animate) {
        if (this.checked == checked && (!animate || animator == null)) {
            this.checked = checked;
            anim = checked ? 1f : 0f;
            invalidate();
            return;
        }
        this.checked = checked;
        float target = checked ? 1f : 0f;
        if (!animate) {
            if (animator != null) {
                animator.cancel();
                animator = null;
            }
            anim = target;
            invalidate();
            return;
        }
        if (animator != null) {
            animator.cancel();
        }
        animator = ValueAnimator.ofFloat(anim, target);
        animator.setDuration(180);
        animator.addUpdateListener(valueAnimator -> {
            anim = (float) valueAnimator.getAnimatedValue();
            invalidate();
        });
        animator.start();
    }

    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        float radius = dp(6);
        track.set(dp(1), dp(1), getWidth() - dp(1), getHeight() - dp(1));

        paint.setStyle(Paint.Style.FILL);
        paint.setColor(TRACK);
        canvas.drawRoundRect(track, radius, radius, paint);

        float half = track.width() / 2f;
        float highlightWidth = half - dp(4);
        float travel = track.width() - highlightWidth - dp(4);
        float left = track.left + dp(2) + travel * anim;
        active.set(left, track.top + dp(2), left + highlightWidth, track.bottom - dp(2));
        paint.setColor(ACTIVE);
        canvas.drawRoundRect(active, radius, radius, paint);

        paint.setStyle(Paint.Style.STROKE);
        paint.setStrokeWidth(dp(1));
        paint.setColor(BORDER);
        canvas.drawRoundRect(track, radius, radius, paint);

        paint.setStyle(Paint.Style.FILL);
        paint.setTypeface(Typeface.DEFAULT_BOLD);
        paint.setTextSize(dp(16));
        paint.setTextAlign(Paint.Align.CENTER);
        float textY = track.centerY() - (paint.ascent() + paint.descent()) / 2f;
        paint.setColor(checked ? MUTED : TEXT);
        canvas.drawText("White list", track.left + half / 2f, textY, paint);
        paint.setColor(checked ? TEXT : MUTED);
        canvas.drawText("Black list", track.left + half + half / 2f, textY, paint);
    }

    @Override
    public boolean onTouchEvent(MotionEvent event) {
        if (event.getAction() == MotionEvent.ACTION_UP) {
            boolean next = event.getX() > getWidth() / 2f;
            if (next != checked) {
                setChecked(next, true);
                if (listener != null) {
                    listener.onModeChanged(next);
                }
            }
            performClick();
            return true;
        }
        return true;
    }

    @Override
    public boolean performClick() {
        super.performClick();
        return true;
    }

    private float dp(float value) {
        return value * density;
    }
}
