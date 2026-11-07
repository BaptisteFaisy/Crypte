// Crypte — post-traitement Ghostty pour kiro-horror-tui.
//
// Vignette chaude + flamme de bougie + grain très léger. Chaque pixel est
// lu à sa propre position (aucun décalage, aucun flou) : le pixel art et
// le texte restent nets au pixel près.

float grain(vec2 p) {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec4 col = texture(iChannel0, uv);

    // Vignette ovale, lumière posée un peu au-dessus du centre.
    vec2 d = (uv - vec2(0.5, 0.42)) * vec2(1.0, 1.25);
    float light = smoothstep(0.95, 0.22, length(d));

    // Flamme : respiration lente + petits sursauts.
    float t = iTime;
    float flame = 0.95 + 0.035 * sin(t * 1.7) * sin(t * 0.63 + 1.3) + 0.012 * sin(t * 17.0);

    vec3 rgb = col.rgb * mix(0.70, 1.0, light) * flame;
    rgb += vec3(0.012, 0.003, 0.0) * (1.0 - light);
    rgb += (grain(fragCoord + fract(t) * 97.0) - 0.5) * 0.016;
    fragColor = vec4(rgb, col.a);
}
