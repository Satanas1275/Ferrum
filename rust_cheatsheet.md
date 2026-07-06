# 🦀 Rust — Fiche complète / Complete Cheat Sheet
### FR 🇫🇷 / EN 🇬🇧

---

## 📦 Cargo — commandes essentielles / essential commands

| Commande | FR | EN |
|---|---|---|
| `cargo new nom` | Crée un nouveau projet | Creates a new project |
| `cargo init` | Initialise Cargo dans un dossier existant | Initializes Cargo in an existing folder |
| `cargo build` | Compile en mode debug | Compiles in debug mode |
| `cargo build --release` | Compile optimisé (lent à compiler, rapide à l'exécution) | Optimized build (slow to compile, fast to run) |
| `cargo run` | Compile + exécute | Compiles + runs |
| `cargo check` | Vérifie que ça compile, sans binaire (rapide) | Checks compilation without building a binary (fast) |
| `cargo test` | Lance les tests | Runs tests |
| `cargo add nom_crate` | Ajoute une dépendance | Adds a dependency |
| `cargo doc --open` | Génère et ouvre la doc du projet | Generates and opens project docs |
| `cargo fmt` | Formate le code automatiquement | Auto-formats code |
| `cargo clippy` | Linter (conseils de bonnes pratiques) | Linter (best-practice suggestions) |

---

## 1. Variables & types de base / Variables & basic types

**FR :** Par défaut, une variable est **immuable** (non modifiable). Il faut `mut` pour la rendre modifiable. C'est un choix volontaire de Rust pour éviter les bugs liés aux modifications accidentelles.

**EN:** By default, a variable is **immutable**. You need `mut` to make it mutable. This is a deliberate Rust design choice to prevent bugs from accidental modification.

```rust
fn main() {
    let x = 5;        // immuable / immutable
    let mut y = 5;     // modifiable / mutable
    y = 6;              // OK
    // x = 6;           // ❌ erreur de compilation / compile error

    // Types de base / basic types
    let entier: i32 = 42;          // entier signé 32 bits / signed 32-bit int
    let non_signe: u32 = 42;        // entier non signé / unsigned int
    let flottant: f64 = 3.14;        // nombre à virgule / floating point
    let booleen: bool = true;
    let caractere: char = '🦀';      // un seul caractère unicode / single unicode char
    let texte: &str = "salut";      // string slice (référence)
    let texte_owned: String = String::from("salut"); // String possédée / owned String

    println!("{x} {y} {entier} {flottant}");
}
```

**Types entiers disponibles / Available integer types :**
`i8 i16 i32 i64 i128 isize` (signés/signed) — `u8 u16 u32 u64 u128 usize` (non signés/unsigned)

**Shadowing (FR) :** on peut redéclarer une variable avec `let` en réutilisant le même nom — ce n'est pas une mutation, c'est une nouvelle variable qui masque l'ancienne.
**Shadowing (EN):** you can redeclare a variable with `let` reusing the same name — this isn't mutation, it's a new variable shadowing the old one.

```rust
let x = 5;
let x = x + 1;      // nouvelle variable, x = 6 / new variable, x = 6
let x = x * 2;      // x = 12
```

---

## 2. Ownership (propriété) — LE concept central de Rust

**FR :** C'est le mécanisme qui permet à Rust de gérer la mémoire **sans garbage collector** et **sans risque de bugs mémoire**, à la compilation. Trois règles :
1. Chaque valeur a **un seul propriétaire** (une variable).
2. Quand le propriétaire sort de portée (`{ }`), la valeur est libérée.
3. Il ne peut y avoir qu'**un seul propriétaire à la fois**.

**EN:** This is the mechanism that lets Rust manage memory **without a garbage collector** and **without memory bugs**, checked at compile time. Three rules:
1. Each value has **one owner** (a variable).
2. When the owner goes out of scope (`{ }`), the value is dropped.
3. There can only be **one owner at a time**.

```rust
fn main() {
    let s1 = String::from("hello");
    let s2 = s1;              // s1 est "déplacé" (moved) vers s2
                                // s1 is "moved" into s2
    // println!("{s1}");      // ❌ erreur : s1 n'est plus valide
                                // ❌ error: s1 is no longer valid
    println!("{s2}");         // ✅ OK

    // Pour les types simples (i32, bool, char...) c'est une COPIE, pas un move
    // For simple types (i32, bool, char...) it's a COPY, not a move
    let a = 5;
    let b = a;      // copie, a reste valide / copy, a stays valid
    println!("{a} {b}"); // ✅ OK les deux
}
```

**FR — pourquoi ça existe :** ça évite les "double free" (libérer 2x la même mémoire) et les "use after free" (utiliser une mémoire déjà libérée), des bugs très courants en C/C++.

**EN — why it exists:** it prevents "double free" (freeing the same memory twice) and "use after free" bugs, very common in C/C++.

### Borrowing (emprunt) / Borrowing

**FR :** Plutôt que de transférer la propriété, on peut **emprunter** une valeur avec `&` (référence). On peut avoir soit plusieurs emprunts immuables, soit un seul emprunt mutable — jamais les deux en même temps.

**EN:** Instead of transferring ownership, you can **borrow** a value with `&` (reference). You can have either multiple immutable borrows, or one single mutable borrow — never both at once.

```rust
fn main() {
    let s = String::from("hello");

    let len = calculer_longueur(&s);   // on emprunte s, on ne le déplace pas
                                          // we borrow s, we don't move it
    println!("'{s}' fait {len} caractères"); // s toujours valide ici / s still valid here

    let mut s2 = String::from("hello");
    ajouter_monde(&mut s2);            // emprunt mutable / mutable borrow
    println!("{s2}");
}

fn calculer_longueur(s: &String) -> usize {
    s.len()
} // s (la référence) sort de portée ici, mais pas la donnée pointée
  // s (the reference) goes out of scope here, but not the pointed-to data

fn ajouter_monde(s: &mut String) {
    s.push_str(" world");
}
```

**Règles d'emprunt / Borrowing rules :**
- ✅ Plusieurs `&T` en même temps / Multiple `&T` at once
- ✅ Un seul `&mut T` à la fois / Only one `&mut T` at a time
- ❌ Un `&T` et un `&mut T` en même temps / A `&T` and a `&mut T` at the same time

---

## 3. Structs (structures)

**FR :** Permet de regrouper des données liées entre elles, comme une classe sans méthodes (les méthodes sont ajoutées à part avec `impl`).

**EN:** Lets you group related data together, like a class without methods (methods are added separately with `impl`).

```rust
struct Joueur {
    nom: String,
    vie: i32,
    niveau: u8,
}

impl Joueur {
    // "constructeur" par convention / constructor by convention
    fn nouveau(nom: &str) -> Joueur {
        Joueur {
            nom: nom.to_string(),
            vie: 100,
            niveau: 1,
        }
    }

    // méthode qui emprunte self / method borrowing self
    fn afficher(&self) {
        println!("{} - vie: {} - niveau: {}", self.nom, self.vie, self.niveau);
    }

    // méthode qui modifie self / method that mutates self
    fn subir_degats(&mut self, degats: i32) {
        self.vie -= degats;
    }
}

fn main() {
    let mut joueur = Joueur::nouveau("Bastian");
    joueur.afficher();
    joueur.subir_degats(20);
    joueur.afficher();
}
```

---

## 4. Enums & pattern matching

**FR :** Un enum représente une valeur qui peut être **une chose parmi plusieurs possibilités**. En Rust, les enums peuvent transporter des données, ce qui les rend très puissants (bien plus qu'en C/Java).

**EN:** An enum represents a value that can be **one of several possibilities**. In Rust, enums can carry data, making them much more powerful than in C/Java.

```rust
enum Direction {
    Nord,
    Sud,
    Est,
    Ouest,
}

enum Message {
    Quitter,                       // pas de données / no data
    Deplacer { x: i32, y: i32 },     // données nommées / named data
    Ecrire(String),                 // une seule donnée / single value
    Couleur(u8, u8, u8),             // tuple de données / tuple data
}

fn traiter(msg: Message) {
    match msg {
        Message::Quitter => println!("Au revoir"),
        Message::Deplacer { x, y } => println!("Déplacement vers {x},{y}"),
        Message::Ecrire(texte) => println!("Message : {texte}"),
        Message::Couleur(r, g, b) => println!("RGB({r},{g},{b})"),
    }
}
```

**Les deux enums les plus utilisés de Rust / Rust's two most-used enums :**

```rust
// Option<T> — remplace le "null" / replaces "null"
enum Option<T> {
    Some(T),
    None,
}

// Result<T, E> — gestion d'erreurs / error handling
enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

```rust
fn diviser(a: f64, b: f64) -> Option<f64> {
    if b == 0.0 {
        None
    } else {
        Some(a / b)
    }
}

fn main() {
    match diviser(10.0, 2.0) {
        Some(resultat) => println!("Résultat : {resultat}"),
        None => println!("Division par zéro impossible"),
    }

    // if let — syntaxe raccourcie quand un seul cas nous intéresse
    // if let — shorthand syntax when only one case matters
    if let Some(r) = diviser(10.0, 0.0) {
        println!("{r}");
    } else {
        println!("Pas de résultat");
    }
}
```

---

## 5. Gestion d'erreurs / Error handling

**FR :** Rust n'a pas d'exceptions. Les erreurs récupérables passent par `Result<T, E>`, les erreurs fatales par `panic!`.

**EN:** Rust has no exceptions. Recoverable errors go through `Result<T, E>`, fatal errors go through `panic!`.

```rust
use std::fs::File;

fn lire_fichier() -> Result<String, std::io::Error> {
    let contenu = std::fs::read_to_string("fichier.txt")?; // ? = propage l'erreur / propagates the error
    Ok(contenu)
}

fn main() {
    match lire_fichier() {
        Ok(contenu) => println!("{contenu}"),
        Err(e) => println!("Erreur : {e}"),
    }

    // .unwrap() = panique si erreur (à éviter en prod)
    // .unwrap() = panics on error (avoid in production)
    // .expect("message") = comme unwrap mais avec message custom
    // .expect("message") = like unwrap but with a custom message
}
```

**FR — le `?` :** C'est un raccourci énorme : si le résultat est `Err`, la fonction retourne immédiatement cette erreur. Ça évite d'écrire un `match` à chaque appel.

**EN — the `?` operator:** A huge shortcut: if the result is `Err`, the function immediately returns that error. Avoids writing a `match` on every call.

---

## 6. Collections courantes / Common collections

```rust
fn main() {
    // Vec<T> — tableau dynamique / dynamic array
    let mut nombres: Vec<i32> = Vec::new();
    nombres.push(1);
    nombres.push(2);
    let nombres2 = vec![1, 2, 3]; // macro pratique / convenient macro

    for n in &nombres2 {
        println!("{n}");
    }

    // HashMap<K, V> — dictionnaire / dictionary
    use std::collections::HashMap;
    let mut scores: HashMap<String, i32> = HashMap::new();
    scores.insert(String::from("Bastian"), 100);

    if let Some(score) = scores.get("Bastian") {
        println!("Score : {score}");
    }
}
```

---

## 7. Traits (un peu comme des interfaces)

**FR :** Un trait définit un comportement partagé que plusieurs types peuvent implémenter. Similaire aux interfaces en Java/TS, ou aux protocols en Swift.

**EN:** A trait defines shared behavior that multiple types can implement. Similar to interfaces in Java/TS, or protocols in Swift.

```rust
trait Animal {
    fn nom(&self) -> String;

    // méthode par défaut / default method
    fn crier(&self) -> String {
        String::from("...")
    }
}

struct Chien;
struct Chat;

impl Animal for Chien {
    fn nom(&self) -> String { String::from("Chien") }
    fn crier(&self) -> String { String::from("Wouf!") }
}

impl Animal for Chat {
    fn nom(&self) -> String { String::from("Chat") }
    // crier() utilise l'implémentation par défaut / uses default implementation
}

fn presenter(animal: &impl Animal) {
    println!("{} dit {}", animal.nom(), animal.crier());
}

fn main() {
    presenter(&Chien);
    presenter(&Chat);
}
```

---

## 8. Vocabulaire clé / Key vocabulary

| Terme | FR | EN |
|---|---|---|
| `&` | Référence (emprunt) | Reference (borrow) |
| `&mut` | Référence modifiable | Mutable reference |
| `mut` | Rend une variable modifiable | Makes a variable mutable |
| Ownership | Un seul propriétaire pour une valeur | One owner per value |
| Borrow checker | Le vérificateur qui applique les règles d'emprunt à la compilation | The checker enforcing borrow rules at compile time |
| `impl` | Bloc où on définit les méthodes d'un type | Block where you define a type's methods |
| `dyn` | Type dynamique (dispatch à l'exécution) | Dynamic type (runtime dispatch) |
| Lifetime (`'a`) | Durée de validité d'une référence | How long a reference stays valid |
| Crate | Un package/bibliothèque Rust | A Rust package/library |
| Panic | Erreur fatale qui arrête le programme | Fatal error that stops the program |

---

## 🎯 Prochaine étape suggérée / Suggested next step

**FR :** Une fois ces bases digérées, les **lifetimes** (`'a`) et les **closures** sont les deux derniers gros morceaux avant de te sentir à l'aise en Rust. Ensuite, direction Bevy pour ton jeu 3D 🦀🎮

**EN:** Once these basics are digested, **lifetimes** (`'a`) and **closures** are the last two big pieces before feeling comfortable in Rust. Then, on to Bevy for your 3D game 🦀🎮
