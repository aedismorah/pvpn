package com.pvpn.app;

import android.animation.ValueAnimator;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.util.AttributeSet;
import android.view.MotionEvent;
import android.view.View;

/** Square on/off switch with the same green styling and animation as ModeSwitchView. */
public class SquareSwitchView extends View {
    private static final int TRACK_OFF = 0xFF172026;
    private static final int TRACK_ON = 0xFF14564C;
    private static final int BORDER = 0xFF22343A;
    private static final int KNOB = 0xFFE8FFF8;
    private static final int KNOB_OFF = 0xFF8BA29E;

    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final RectF track = new RectF();
    private final RectF knob = new RectF();
    private final float density;
    private boolean checked;
    private float anim;
    private ValueAnimator animator;

    public interface OnCheckedChangedListener {
        void onCheckedChanged(boolean checked);
    }

    private OnCheckedChangedListener listener;

    public SquareSwitchView(Context context) {
        this(context, null);
    }

    public SquareSwitchView(Context context, AttributeSet attrs) {
        super(context, attrs);
        density = getResources().getDisplayMetrics().density;
        setClickable(true);
    }

    public void setOnCheckedChangedListener(OnCheckedChangedListener listener) {
        this.listener = listener;
    }

    public void setChecked(boolean checked, boolean animate) {
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
        float radius = dp(2);
        track.set(dp(1), dp(1), getWidth() - dp(1), getHeight() - dp(1));

        paint.setStyle(Paint.Style.FILL);
        paint.setColor(TRACK_OFF);
        canvas.drawRoundRect(track, radius, radius, paint);

        float knobSize = track.height() - dp(6);
        float left = track.left + dp(3) + (track.width() - knobSize - dp(6)) * anim;
        knob.set(left, track.top + dp(3), left + knobSize, track.bottom - dp(3));

        paint.setColor(checked ? TRACK_ON : TRACK_OFF);
        canvas.drawRoundRect(track, radius, radius, paint);
        paint.setColor(checked ? KNOB : KNOB_OFF);
        canvas.drawRoundRect(knob, radius, radius, paint);

        paint.setStyle(Paint.Style.STROKE);
        paint.setStrokeWidth(dp(1));
        paint.setColor(BORDER);
        canvas.drawRoundRect(track, radius, radius, paint);
    }

    @Override
    public boolean onTouchEvent(MotionEvent event) {
        if (event.getAction() == MotionEvent.ACTION_UP) {
            boolean next = !checked;
            setChecked(next, true);
            if (listener != null) {
                listener.onCheckedChanged(next);
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
