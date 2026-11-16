fn main() {
    rune::run(rune::AppSpec::<()>::new("Sound Tools", (1470.0, 920.0), || (), |_, _| rune::el()));
}
