# Elfie

This is the main source code repository for [Elfie][1]. It contains the specification details, compiler, and standard library.

## Why Elfie Exists?

When you try and code by simply prompting your agent, you're relying on the internals of the LLM and whatever guardrails you have in place to transform your non-deterministic input into deterministic output. And with the increase in volume of code changes coming out of LLMs, more and more people are abondoning reviewing their outputs and simply trying to trust their agents harness and hoping you've put the right pieces in place. It tries to treat the LLM as an engineer.

Elfie takes a completely different strategy and instead treats the LLM as a compiler.

When working with a compiled language, you don't usually care exactly what the outputted binaries or assembly instructions look like. Instead, the thing of primary importance is that the code you write compiles to something that works exactly as defined and expected. And internally in the compiler it will work to translate to a given language, optimize code, create mappings, manage memory, and do any number of things to improve the output.

In treating the LLM as a compiler, it means that all input must be deterministic and structured. But in exchange it does all of those things and you can care less about the details and more about ensuring what you write is correct.

### Benefits

- Agentic development with predictable, repeatable, and iterable results
- Familiar patterns and paradigms that build upon existing principles of good system design
- A programming language with features specifically created for agentic development
- Gain improvements in output with compiler updates, not more prompts or skills
- Flexbile language targetting mean a single code base can be used across interfaces and platforms

## How Elfie Works

Elfie builds on top of existing coding paradigms like variables and `for` loops and extends these concepts to the next higher order of conceptualization. It combines standard programming, meta programming, and agentic programming to create a platform on which you have greater control over the structure of the code and the output, while still staying within familiar territory. During compilation, Elfie behaves like a compiler, an interpretter, and an MCP server in order to maximize capabilities.

But that's all just a bunch of flowery words. The key piece to remember is that everything in the language has value layer, a context layer, and a scope.

### Value Layer

This is the layer that will feel more familiar to folks familiar with more classical programming languages. This is the layer that defines the actual "value" of an item. For example, in `const x = 2;`, the value of `x` is `2`. This layer is where most standard programming will happen. The value layer is accessed either directly with the variable, or with the `.` accessor. E.g. `foo.hello = 'world'`.

### Context Layer

The context layer will still seem somewhat familiar to those familiar with reflective programming. This is the layer in which the context for the system is defined, and it provides necessary information for agents to be able to translate code effectively. Using that same example, with `const x = 2;` the value `x` has a `type` of `number`, which you can access with `x@type`. The `@` keyword accesses context for the object, and allows for specific operations.

This layer is most helpful when it comes to the agentic instructions layer. All variables, functions and other objects in the code can have a description, a type, and various other properties that the agent can access as it writes its code. This layer is unlikley to transfer directly to code and is more likely to be seen in tests, documentation, or broader patterns of logic.

### Scope

The scope layer will feel familiar in the abstract as a programming concept, but is made more real in Elfie. If you are familiar with meta programming, this will seem familiar. Additionally, if you are used to working with Entity Component System code or using composition over inheritance style of programming, there will be some parallels there as well. While less relevant (though still usable) with our examples previously, it becomes a lot more relevant when it comes to functions and data structures and when using a `trait`. The scope layer can be accessed with the `$` accessor (e.g. `x$foo`).

In the scope layer, you can access and modify the scope of a function, variable, or object dynamically based on your needs. Most often this will be helpful with `trait`'s. A `trait` allows you to create reusable components or pieces of logic that can be shared across the system. For example, with the code `trait hasName { $name = string; }`, you can define a reusable trait that adds a `name` variable to the current scope.

[1]: https://elfie.dev
