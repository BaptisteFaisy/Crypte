// Crypt — Ghostty post-processing for kiro-horror-tui.
//
// Warm vignette + candle flame + very light grain. Every pixel is read
// at its own position (no offset, no blur): pixel art and text stay
// pixel-sharp.

float grain(vec2 p) {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec4 col = texture(iChannel0, uv);

    // Oval vignette, light set slightly above center.
    vec2 d = (uv - vec2(0.5, 0.42)) * vec2(1.0, 1.25);
    float light = smoothstep(0.95, 0.22, length(d));

    // Flame: slow breathing + small flickers.
    float t = iTime;
    float flame = 0.95 + 0.035 * sin(t * 1.7) * sin(t * 0.63 + 1.3) + 0.012 * sin(t * 17.0);

    vec3 rgb = col.rgb * mix(0.70, 1.0, light) * flame;
    rgb += vec3(0.012, 0.003, 0.0) * (1.0 - light);
    rgb += (grain(fragCoord + fract(t) * 97.0) - 0.5) * 0.016;
    fragColor = vec4(rgb, col.a);
}
