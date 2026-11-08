// Crypt — Ghostty post-processing for kiro-horror-tui.
//
// Static warm vignette. Every pixel is read at its own position
// (no offset, no blur): pixel art and text stay pixel-sharp.
// No animation: no flame, no grain.

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec4 col = texture(iChannel0, uv);

    // Oval vignette, light set slightly above center.
    vec2 d = (uv - vec2(0.5, 0.42)) * vec2(1.0, 1.25);
    float light = smoothstep(0.95, 0.22, length(d));

    vec3 rgb = col.rgb * mix(0.70, 1.0, light);
    rgb += vec3(0.012, 0.003, 0.0) * (1.0 - light);
    fragColor = vec4(rgb, col.a);
}
