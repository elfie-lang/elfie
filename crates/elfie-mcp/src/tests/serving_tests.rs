// @lfy def/mcp/main.lfy:serve
//! Tests of `serve` and `host`: which tools a session is offered, how a call reaches the
//! fn it names, and what a hosted session is bound to.

use super::*;
use crate::serving::{LOOPBACK, Served, offered};

#[test]
fn the_tools_are_listed_with_their_arguments() {
    let names: Vec<String> = tools().into_iter().map(|registered| registered.tool.name).collect();
    assert_eq!(names, EVERY_TOOL);
    // The tools offered to a session over standard input and output are every fn of
    // this module that carries `tool` but `report` and `reportMany`.
    // @lfy def/mcp/main.lfy:serve#serve:serve:fae469e7dbfdef868655e96269f387d1ceb2373d31a063ce8c099b6960d6a06a
    let fixture = Fixture::new();
    assert_eq!(offered(&tools(), &fixture.session().workspace, None), TOOL_NAMES);
    let served = Served::over_stdio(&fixture.root);
    // Such a session is bound to nothing.
    // @lfy def/mcp/main.lfy:serve#serve:serve:8579f2d17bf0b508fda59fe780ac73d54d3271fe8f9994ccd60bd5f66d026331
    assert!(served.session.lock().unwrap().binding.is_none());
    let result = served.list();
    // The result may be treated as fresh for no time and cached by this client only.
    // @lfy def/mcp/main.lfy:problems#problems:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    assert_eq!(result.ttl_ms, Some(0));
    assert_eq!(result.cache_scope, Some(CacheScope::Private));
    let listed = result.tools;
    assert_eq!(listed.len(), TOOL_NAMES.len());
    // `report` is not offered, so it is not listed.
    // @lfy def/mcp/main.lfy:report#report:tool:ff191fef77edc21f5b1b6379da11a026007625ceb2ea84877365d76b43d3a88f
    assert!(!listed.iter().any(|tool| tool.name == "elfie_report"), "{listed:?}");
    // Nor is `reportMany`, which is not offered either.
    assert!(!listed.iter().any(|tool| tool.name == "elfie_reportMany"), "{listed:?}");
    // Every other tool is listed as one tool, named and described by its fn.
    // @lfy def/mcp/main.lfy:problems#problems:tool:47dafd874509841945f72721454f0bde82f7cb760967d4104944c32a0b08e4f6
    assert_eq!(listed[0].name, "elfie_problems");
    // The one parameter of `problems` that is not the session is optional, so its
    // argument is not required; the one of `find` is neither optional nor defaulted.
    // @lfy def/mcp/main.lfy:problems#problems:tool:87fb1f093e523c06d8bd452dd3daa9a60adcb2fa4c4dd899726161be0c185d3c
    assert_eq!(listed[0].input_schema["required"], serde_json::json!([]));
    // @lfy def/mcp/main.lfy:find#find:tool:26960caba8b776d48816a31b7cd3d9d20254e141db7595c95945236688b3ff13
    assert_eq!(listed[1].input_schema["required"], serde_json::json!(["name"]));
    // The description of an argument is the description of its parameter.
    // @lfy def/mcp/main.lfy:problems#problems:tool:0d47cd7405248873ce91f19150020622e2bbc3156320aa272d9325d8ab7ad9b4
    assert_eq!(
        listed[0].input_schema["properties"]["file"]["description"],
        "a path relative to the root; every file when left out"
    );
    // The name of each tool is `elfie_` and the fn's identifier, and its description the
    // fn's definition.
    // @lfy def/mcp/main.lfy:problems#problems:tool:e3326a06fa53e9623634da620a9bac4aa2a1d0c7e0ea9f0d1143f0f4443b8ba5
    // @lfy def/mcp/main.lfy:problems#problems:tool:97369ca270f9c5503c620a5a45c2e5bda573d9d44900f052d6aa2f6fab3ccd0c
    assert_eq!(
        listed[0].description.as_deref(),
        Some("The problems of the program, or of one file: what the compiler, an editor, or a check would report")
    );
    // A tool whose fn takes nothing but the session holds no argument.
    // @lfy def/mcp/main.lfy:grammar#grammar:tool:7c4b82dcd83184afd88477db1f841b99e09a48579fbd8a607ecacab9384b07a8
    assert_eq!(listed[5].input_schema["properties"], serde_json::json!({}));
    // A line is a number, and is listed as one; `changes` now takes a target too.
    // @lfy def/mcp/main.lfy:source#source:tool:c6487b689eaab2ccd0e199ee1b320e9a2665cee02bdd6cb984e65e94e9b22b46
    assert_eq!(listed[11].input_schema["properties"]["line"]["type"], "number");
    // @lfy def/mcp/main.lfy:source#source:tool:5bf72ec14e48137ed89a57ea542187e7e9c485c8a42ba64d90d18cc298f7a0fe
    assert_eq!(listed[11].input_schema["required"], serde_json::json!(["file", "line"]));
    // @lfy def/mcp/main.lfy:changes#changes:tool:ec3c7defff9f765cc593846440341f6c9ab6818d3c051ab9101385709365d3fb
    assert_eq!(listed[12].input_schema["properties"]["target"]["type"], "string");
    // @lfy def/mcp/main.lfy:changes#changes:tool:0c631194a5026a108cc79d09dc3877b6278e686a4a4b0853d4b441b65c8bb6fc
    assert_eq!(listed[12].input_schema["required"], serde_json::json!(["unit"]));
    // `text` is a boolean with a default, so it is listed as one and is not required.
    // @lfy def/mcp/main.lfy:output#output:tool:b44f0f08cd4a5a17ed3563ee169631ec0e06701a1866f2bcbb070d4ea652c7d5
    // @lfy def/mcp/main.lfy:output#output:tool:cccf2bf991122a41e5f8d1b615ff93f9a9b92d85f03790b2610579b50c0becde
    assert_eq!(listed[9].input_schema["properties"]["text"]["type"], "boolean");
    assert_eq!(listed[9].input_schema["required"], serde_json::json!(["name"]));
    // @lfy def/mcp/main.lfy:output#output:tool:d72cb21d0e903b84ed1802adf513079764243da9561991d7767a50a7eb2e2cbd
    assert_eq!(
        listed[9].description.as_deref(),
        Some(
            "Where generated code is, with its text or without it: every region that came from one entity, that answers for one criterion or test, or that one output file holds"
        )
    );
    // `search` carries `tool` alone, so a session over standard input and output is
    // offered it; the fns carrying `compilerTool` and `commandTool` are not listed,
    // since such a session has no binding.
    // @lfy def/mcp/main.lfy:search#search:tool:38ffd83bb0a3a18ad44b57a02e041c06b560ae946cbab843e22f39a99e9aac66
    // @lfy def/mcp/main.lfy:search#search:tool:49b02b5a6852ab0082ddca33e30f1e4557cf5670633599b4da7d94b54afc74a0
    assert_eq!(listed[10].name, "elfie_search");
    // @lfy def/mcp/main.lfy:search#search:tool:84b35a27f4c75349de61c3a20a30bb45eead886577edc895af4e407810c893a3
    // @lfy def/mcp/main.lfy:search#search:tool:1e8cb253167c19b0e2e9148d8b4dd61e332b68d5b13ccf6aa0523572759ef678
    // @lfy def/mcp/main.lfy:search#search:tool:32aab10a54d3615f163e0fbc4cd259bb8fdce07cc79bde7fddfbe0fa16fba954
    assert_eq!(listed[10].input_schema["required"], serde_json::json!(["pattern"]));
    // @lfy def/mcp/main.lfy:task#task:tool:b4a5feee0df4f25397442a14ddc5789cddaecd15441de63010655c2768703e25
    // @lfy def/mcp/main.lfy:dependency#dependency:tool:c19bc9c85c771a6a121e931a3d90f5e925e08c8d87bdaf800db6922b0e2876ff
    // @lfy def/mcp/main.lfy:mark#mark:tool:d0022315029fb169381cd2230a938407632d7d0dd0131fbc83073feec1ae22af
    // @lfy def/mcp/main.lfy:submit#submit:tool:8f4adcbdfc16d749d9b83d55424fdb414dca3efc886559174e902fd2a4912a79
    // @lfy def/mcp/main.lfy:build#build:tool:88906b89a3fff75afd55acabcbfcfcad2e07d893ba2ff2c1a9fbe35abfcb1f8d
    // @lfy def/mcp/main.lfy:test#test:tool:3095733c477eec37202566f8813793d01f848b595701cdd835ffc630ba78f768
    // @lfy def/mcp/main.lfy:lint#lint:tool:f0044ba556ab8e00066360dce81c2a584d6ad207e30d0233e25d4294fde9e8a5
    // @lfy def/mcp/main.lfy:format#format:tool:fad2173ad6add7bf6e4a61983e9685caf2ffe4dfb3b1a4887c920667e347acee
    for withheld in ["elfie_task", "elfie_dependency", "elfie_mark", "elfie_submit", "elfie_build", "elfie_test", "elfie_lint", "elfie_format"] {
        assert!(!listed.iter().any(|tool| tool.name == withheld), "{withheld}");
    }
}

/// Every fn carrying `tool` is listed with the name, description, arguments, and
/// required arguments its parameters give it, so each of them is one tool.
// @lfy def/mcp/main.lfy:task#task:tool:b4176adea23f8e6c58f4d041e7a85447286076984bd05464e92528b60fda6d8b
// @lfy def/mcp/main.lfy:task#task:tool:46ad78939d73914a576d0dce0904cae992474f9eeb184c717a5a0c324091d4d5
// @lfy def/mcp/main.lfy:task#task:tool:3f09d52a0b71d1b830863b88454d30b98bb821b36e428efed32d7e6169482a3c
// @lfy def/mcp/main.lfy:task#task:tool:6ddac2bcba95be32011edabc6d0abbc63da57cd4a0182a8c86f882a8482706e7
// @lfy def/mcp/main.lfy:task#task:tool:e91db1a6069241eefc991fb1723f76dac6a194f33db9dbeaa21f8163f69cbdaf
// @lfy def/mcp/main.lfy:task#task:tool:b2e2fadd9a1cd573a4b55e2e77fbd573ec3a9f6184ccd88beca5e743b871e9cd
// @lfy def/mcp/main.lfy:task#task:tool:b361de400635b42112cf8a02205ed19f3875a7f7f9835b646817814b39f0233d
// @lfy def/mcp/main.lfy:dependency#dependency:tool:4fd4dbdb36a4d1d4e8a9589471a29d8337372b850160162b8a14f940ad74bbc6
// @lfy def/mcp/main.lfy:dependency#dependency:tool:7b7fe0f695f34ca950fbd8f4bc3aa16eb90bc78d78ed5a87812fe95b03c71eb5
// @lfy def/mcp/main.lfy:dependency#dependency:tool:62ccf07aff23acf72a7e091f19b6d110da5bb4dc6f969dcadd130d72ab9ccf3f
// @lfy def/mcp/main.lfy:dependency#dependency:tool:01c082cb2c776a72049a0c164c556b408ada68b6c8d3e961aed6be20476ba114
// @lfy def/mcp/main.lfy:dependency#dependency:tool:feb08658fc3d60e206035d80439de4021e443fe53d2226e6e8ad43298da9f5b7
// @lfy def/mcp/main.lfy:dependency#dependency:tool:4894a15426436158e1d439a6ad82b49784af28fc4c896d683eb4bcfe455480a0
// @lfy def/mcp/main.lfy:dependency#dependency:tool:2b186e60bc3e791246ddc810258418700586e003d4998497bd580b40f6624761
// @lfy def/mcp/main.lfy:mark#mark:tool:0f8e6f40a5d6de867cde8b1be5b3fc853845b786664671c550f9dad09eb01911
// @lfy def/mcp/main.lfy:mark#mark:tool:0b9567706ca9a8b66a0757d097e5fd9c7e6df936098d4aeb7f7e6834a6635d71
// @lfy def/mcp/main.lfy:mark#mark:tool:5295f5acf77612da1ff8627c746c87ccff5a0dbb1770c05ed869d1ed28d6c7a7
// @lfy def/mcp/main.lfy:mark#mark:tool:44369bf678415e07ca9c08b82b57c5fe7969e2e9850e765a8f32faeab4b784c4
// @lfy def/mcp/main.lfy:mark#mark:tool:644ced5c6aeca4f5ba2d0a64656faf7dc4224aa51d7dcfc2c46b2d9a241fb040
// @lfy def/mcp/main.lfy:mark#mark:tool:84d31d74a162783af16e3e2c1bbb0339be256b810408f3869f52a6d8f243ec5e
// @lfy def/mcp/main.lfy:mark#mark:tool:745f570e06eb5111681f3c579ae61468329b1f2ae73381dfb2d99e3ba743c32f
// @lfy def/mcp/main.lfy:submit#submit:tool:46e6a49028a494b7d6bb872cd0cc63d6d03e25fffce40f625b0facd57b49a725
// @lfy def/mcp/main.lfy:submit#submit:tool:46c97b8c41d5f41394e9c6efbaf1f3bdc24c40082759615fb1b06732fb2d02c7
// @lfy def/mcp/main.lfy:submit#submit:tool:59836951e15ef484cb381d25b49b247ff2da58ae11631e91ff190839871ce24d
// @lfy def/mcp/main.lfy:submit#submit:tool:27e3a76662e3a4a2d2275aa703d2b835bd68c51282753e21b9337c744d37e5d6
// @lfy def/mcp/main.lfy:submit#submit:tool:87847e0f685515fe21c083343037e22e14155b3956e0220e0c6c17c145fd682d
// @lfy def/mcp/main.lfy:submit#submit:tool:f5d375e7f92630c7c75c6af5a62d636df1c895f2abf6280b5fc27486624dd3d6
// @lfy def/mcp/main.lfy:submit#submit:tool:5e8f39a0f84b057b8516d3aba402f08b7a867533e314d8ede4294d9169055ec1
// @lfy def/mcp/main.lfy:search#search:tool:dffd65949512cdddb7735f1aeed21e452cf605acabf06b395717014263c1c4e8
// @lfy def/mcp/main.lfy:search#search:tool:8cabf755d8b4ed9c400ddc760b3676b7d288d84ef6a4b4d3588840ee0fb85e46
// @lfy def/mcp/main.lfy:search#search:tool:502a513b0a0681b5d9af190aa237ed651ee77b387a6b91c00943b77203be2a26
// @lfy def/mcp/main.lfy:build#build:tool:437d060ed5ca74732574dd1afab19a5a166957ef20fcdb97bf4d590041c48336
// @lfy def/mcp/main.lfy:build#build:tool:e0309114048649453e6790ef4c6deeabd6ec9481b22aa5d6159c671535d25832
// @lfy def/mcp/main.lfy:build#build:tool:d0bfe30fbbef86bd5724c6c59435c754542f0b44ae99df590217de4b3e02fd28
// @lfy def/mcp/main.lfy:build#build:tool:a89edaee38beabc1ef0df886f2a21cc45ce985962b64e3309e470a2a02e9b576
// @lfy def/mcp/main.lfy:build#build:tool:a3290c07f19e4214355190476d5ab8cc6a9bba3ec08cfedee6e90f738f4dcfc0
// @lfy def/mcp/main.lfy:build#build:tool:cc821fff828717e6310df1103a08b0267f91fc98bbc71bc89165e1224840cb87
// @lfy def/mcp/main.lfy:build#build:tool:d7aadcec979a675c909a3493228563ee20b979feaf86ec325edc207f6683b4f5
// @lfy def/mcp/main.lfy:test#test:tool:5386b37f95ca729944462379291817c9470858369f6cfb9387b834f8767c2b51
// @lfy def/mcp/main.lfy:test#test:tool:a038afc81a5b6f64a780e0144c3ac1be7ce831938025338a3c18193929918aba
// @lfy def/mcp/main.lfy:test#test:tool:b781017493ee33fccd6be22cef4cb4b2338a130da8607db5c8a6899222f40fd1
// @lfy def/mcp/main.lfy:test#test:tool:4930fab7d3d2c47253afb07544bc3ab8fcd4c5deb1e42ce3f9f8b2dcc6175c30
// @lfy def/mcp/main.lfy:test#test:tool:cbeb0da9f28bfbd6a6d084bb140fca7c043af3a7fee4c0c506d08562fcb13392
// @lfy def/mcp/main.lfy:test#test:tool:91cfa553c86897863866df4f70a520da5ab64eb9facaea1c00b2c552ee52e2da
// @lfy def/mcp/main.lfy:test#test:tool:430d6d90db0096b765c313099b0425e48c0069b5c3a1f92aa82676371e14f7f3
// @lfy def/mcp/main.lfy:lint#lint:tool:6e14d79b29847057cee32aad56ac37159ec40c848c8cb9346ffbcca0cf4ce022
// @lfy def/mcp/main.lfy:lint#lint:tool:9f1b5c7200bd49c99da2c72dd74e0589a75980b35e4b797ecc4b538e18b380a1
// @lfy def/mcp/main.lfy:lint#lint:tool:fada2d3544a4caf7440370d2fdff08fd8d46235a4307ed27758f2aa426047db5
// @lfy def/mcp/main.lfy:lint#lint:tool:f23edb098a47aff31e4aaab809a03e3db1a479cda385597dafd4ffca5fc96a43
// @lfy def/mcp/main.lfy:lint#lint:tool:9293b2f58b28f6819e289105b3ffdfeddf57cb0b003b969596d58a7cf3cedeab
// @lfy def/mcp/main.lfy:lint#lint:tool:8415b8b106d669fec27102fd04507c3d814b919d8bcf79f8e7d2650f5fb4faee
// @lfy def/mcp/main.lfy:lint#lint:tool:81afa3f50b86d1ed54b7bad2244850fb1e841e53d326e18b8f9188e1037bc426
// @lfy def/mcp/main.lfy:format#format:tool:a84a6f1ea99ee3c31c458952672bfa71b4b076970b0ea7e5f97fc4ca1ad3476d
// @lfy def/mcp/main.lfy:format#format:tool:c9525e9b70499077881113750f0011474b3ed51c8f839564643d93637cedd60c
// @lfy def/mcp/main.lfy:format#format:tool:b844517eed0302f020cc276c74d1c8e649070a57de01e61e1bc8194d3157f1a5
// @lfy def/mcp/main.lfy:format#format:tool:6b1377a3f7b741d23cf4ee8f6be9b6e69d161c9b715fd8f55d0247578017bb6c
// @lfy def/mcp/main.lfy:format#format:tool:6f7967f9d4f086abae04ce989ce1d49bea43882ec4bbb31e34aee7bf9417d481
// @lfy def/mcp/main.lfy:format#format:tool:c7c05422e084fd79bb583a5aa90b70d947898019100fe9d2bd60b4fe23a540f2
// @lfy def/mcp/main.lfy:format#format:tool:41a181e4de5b31903fd19b4fec451a4d3982908e194189ef3b45c691280dd06b
#[test]
fn every_tool_is_listed_as_its_fn_declares_it() {
    let listed = traits::list(&tools());
    let of = |name: &str| {
        listed
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("{name} is listed"))
            .clone()
    };
    let task = of("elfie_task");
    assert!(task.description.as_deref().is_some_and(|text| text.starts_with("What this compiler run must do")));
    // The one parameter of `task` that is not the session is optional, so its argument
    // is not required.
    assert_eq!(task.input_schema["required"], serde_json::json!([]));
    assert_eq!(task.input_schema["properties"]["unit"]["type"], "string");
    let dependency = of("elfie_dependency");
    assert_eq!(dependency.input_schema["required"], serde_json::json!(["name"]));
    assert!(
        dependency
            .description
            .as_deref()
            .is_some_and(|text| text.starts_with("What one entity of a dependency"))
    );
    // Every parameter of `mark` is optional, so none of its arguments is required, and
    // `line` is a number.
    let mark = of("elfie_mark");
    assert_eq!(mark.input_schema["required"], serde_json::json!([]));
    assert_eq!(mark.input_schema["properties"]["line"]["type"], "number");
    assert_eq!(mark.input_schema["properties"]["entity"]["type"], "string");
    // Both parameters of `submit` are needed, so both arguments are required.
    let submit = of("elfie_submit");
    assert_eq!(submit.input_schema["required"], serde_json::json!(["outcome", "message"]));
    assert_eq!(submit.input_schema["properties"]["outcome"]["description"], "done, blocked, or clarify");
    let search = of("elfie_search");
    assert_eq!(search.input_schema["required"], serde_json::json!(["pattern"]));
    // The one parameter of `reportMany` that is not the session is needed, so its
    // argument is required, and it is described as its parameter is.
    let report_many = of("elfie_reportMany");
    assert_eq!(
        report_many.description.as_deref(),
        Some("Submit the verdicts on several criteria and tests of the review this session is for, one per line")
    );
    assert_eq!(report_many.input_schema["required"], serde_json::json!(["lines"]));
    assert_eq!(report_many.input_schema["properties"]["lines"]["type"], "string");
    assert_eq!(
        report_many.input_schema["properties"]["lines"]["description"],
        "one JSON object per line, with the keys id, file, line, status, evidence, and note, holding what report would be given; blank lines are skipped"
    );
    for (name, required, arguments) in [
        ("elfie_build", serde_json::json!([]), vec!["scope", "target"]),
        ("elfie_test", serde_json::json!([]), vec!["filter", "scope", "target"]),
        ("elfie_lint", serde_json::json!([]), vec!["scope", "target"]),
        ("elfie_format", serde_json::json!([]), vec!["target"]),
    ] {
        let tool = of(name);
        assert_eq!(tool.input_schema["required"], required, "{name}");
        for argument in arguments {
            assert_eq!(tool.input_schema["properties"][argument]["type"], "string", "{name} {argument}");
        }
        assert!(tool.description.as_deref().is_some_and(|text| text.starts_with("Run the target's")), "{name}");
    }
}

/// The whole server, driven over streams: a root whose `def` holds `a.lfy` reading
/// `const y = z;` answers a `tools/list` naming the fourteen tools and a call of
/// `elfie_problems` with the one binder error at `z`, and returns 0 once the stream it
/// reads as standard input closes.
// @lfy def/mcp/main.lfy:serve#serve:serve:594496b8884ab7d56e00dedca9ef986ab491c41d7a8017be615d11a2edfb0c89
#[tokio::test]
async fn serving_lists_the_tools_answers_a_call_and_returns_zero_when_input_closes() {
    let fixture = Fixture::new();
    fixture.write("def/a.lfy", "const y = z;\n");
    let session = fixture.session();
    let text = problems(&session, None).unwrap();
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.starts_with("def/a.lfy:1:10: binder error: "), "{text}");
    let result = call(&session, "elfie_problems", serde_json::json!({}));
    assert!(!result.is_error);
    assert_eq!(result.text, text);
    let result = call(&session, "elfie_problems", serde_json::json!({ "file": "def/a.lfy" }));
    assert_eq!(result.text, text);

    // @lfy def/mcp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b:2
    let mut agent = Agent::connect(&fixture.root);
    let initialized = agent
        .request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "elfie-mcp test", "version": "0" },
            }),
        )
        .await;
    // @lfy def/mcp/main.lfy:Protocol
    assert_eq!(initialized["result"]["protocolVersion"], "2025-06-18", "{initialized}");
    // Tools only: the capabilities carry no resources and no prompts.
    // @lfy def/mcp/main.lfy:serve#serve:serve:4d105e14d91381963764973d69e9e54abacde103ac17990250a98dadf1ced8cb
    assert!(initialized["result"]["capabilities"]["tools"].is_object(), "{initialized}");
    assert!(initialized["result"]["capabilities"]["resources"].is_null(), "{initialized}");
    assert!(initialized["result"]["capabilities"]["prompts"].is_null(), "{initialized}");
    // The instructions name only the tools offered over standard input and output, and
    // say outright that none of them writes a file.
    // @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
    let said = initialized["result"]["instructions"]
        .as_str()
        .unwrap_or_else(|| panic!("instructions, not {initialized}"));
    for name in TOOL_NAMES {
        assert!(said.contains(name), "{said}");
    }
    for withheld in ["elfie_report", "elfie_task", "elfie_dependency", "elfie_mark", "elfie_submit", "elfie_build"] {
        assert!(!said.contains(withheld), "{said}");
    }
    assert!(said.contains("None of them writes a file."), "{said}");
    agent
        .send(serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;

    // @lfy def/mcp/main.lfy:serve#serve:serve:fae469e7dbfdef868655e96269f387d1ceb2373d31a063ce8c099b6960d6a06a
    let listed = agent.request("tools/list", serde_json::json!({})).await;
    let served: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("a list of tools, not {listed}"))
        .iter()
        .map(|tool| tool["name"].as_str().expect("a tool name"))
        .collect();
    assert_eq!(served, TOOL_NAMES);

    // @lfy def/mcp/main.lfy:problems#problems:tool:d9b6b1ce215d966d4d49ba31d319199d9539fe1614bed20bc86032b0e2fc28c0
    let called = agent
        .request("tools/call", serde_json::json!({ "name": "elfie_problems", "arguments": {} }))
        .await;
    assert_ne!(called["result"]["isError"], serde_json::json!(true), "{called}");
    assert_eq!(called["result"]["content"][0]["text"], text, "{called}");

    // @lfy def/mcp/main.lfy:serve#serve:serve:e36ddce87201b751562fbe8a68504f70e09879c00d4b93be51b40d6176adae2e
    assert_eq!(agent.close().await, 0);
}

// @lfy def/mcp/main.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
#[test]
fn problems_says_when_there_are_none() {
    let fixture = Fixture::new();
    fixture.write("def/a.lfy", "const y = 1;\n");
    let session = fixture.session();
    assert_eq!(problems(&session, None).unwrap(), "no problems");
    assert_eq!(problems(&session, Some("def/a.lfy")).unwrap(), "no problems in def/a.lfy");
    // A file the program does not hold has no problems either, and is answered with
    // text rather than a failure.
    // @lfy def/mcp/main.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
    assert_eq!(problems(&session, Some("def/b.lfy")).unwrap(), "no problems in def/b.lfy");
}

// The session over standard input and output is offered both `find` and `outline`, so
// an argument of either that is missing, unknown, or of the wrong type is an error
// naming it and the fn is not called.
// @lfy def/mcp/main.lfy:find#find:tool:14b1732157522ebf4b24b6038fcddb65b5b23a3689651521b1bf763ca8fb0879
// @lfy def/mcp/main.lfy:find#find:tool:26dc105fcfc8d9cc4ed181d1ae1172c0d0e88942453e1513fd22d3d6c8b3b095
#[test]
fn a_bad_argument_is_an_error_naming_it() {
    let fixture = Fixture::new();
    fixture.write("def/a.lfy", "const y = 1;\n");
    let session = fixture.session();
    let result = call(&session, "elfie_find", serde_json::json!({}));
    assert!(result.is_error);
    assert!(result.text.contains("name"), "{}", result.text);
    let result = call(&session, "elfie_find", serde_json::json!({ "name": "y", "extra": true }));
    assert!(result.is_error);
    assert!(result.text.contains("extra"), "{}", result.text);
    // @lfy def/mcp/main.lfy:outline#outline:tool:6e95ab5fd88f41d7145f86ed344f8156d3464326e598b0ac637ac3ddbc5d369f
    // @lfy def/mcp/main.lfy:outline#outline:tool:150963511f1c73e51401709017f75ec80e37e411880f968dadf301ed995494cd
    let result = call(&session, "elfie_outline", serde_json::json!({ "file": 1 }));
    assert!(result.is_error);
    assert!(result.text.contains("file") && result.text.contains("string"), "{}", result.text);
    // The fn fails: an error carrying the failure, and the server goes on, which the
    // call after this one shows.
    // @lfy def/mcp/main.lfy:outline#outline:tool:6114e21db6cb9375ba623e9ff0b359857133d98565b50e37b08ee737a3e0ac41
    // @lfy def/mcp/main.lfy:outline#outline:tool:532b2ad88427069a8c24f3a734fe21c29f9d0b4bcf57c523a2436c4d0228712f
    let result = call(&session, "elfie_outline", serde_json::json!({ "file": "def/none.lfy" }));
    assert!(result.is_error);
    assert!(result.text.contains("def/a.lfy"), "{}", result.text);
    // The fn returns: the result it returns is the call's result, and the fn was reached
    // with the session and the arguments.
    // @lfy def/mcp/main.lfy:outline#outline:tool:56807c149fa490c87e28f903d14a7c0f45d7bd9ef10df81481e83b233f241b30
    // @lfy def/mcp/main.lfy:outline#outline:tool:f5e2e8d44cd795aa69322fef5a2f21e8c943fd631ec83fb04f50665b2a0d412f
    let result = call(&session, "elfie_outline", serde_json::json!({ "file": "def/a.lfy" }));
    assert!(!result.is_error, "{}", result.text);
    assert_eq!(result.text, outline(&session, "def/a.lfy").unwrap());
}

// @lfy def/mcp/main.lfy:serve#serve:serve:995535b9c7a11554f96d88793004bc092de31525b26cd0ca15ce449e17d8e386
#[test]
fn changed_vanished_and_new_files_are_reread_before_a_call() {
    let fixture = Fixture::new();
    fixture.write("def/a.lfy", "const y = z;\n");
    let mut session = fixture.session();
    assert!(problems(&session, None).unwrap().contains("binder error"));

    // A file whose modification time or size differs is re-read.
    // @lfy def/mcp/main.lfy:serve#serve:serve:1893a7a1afe4f16bc76a47596614b6645e060072995b9387c9386905037fe959
    fixture.write("def/a.lfy", "const z = 1;\nconst y = z;\n");
    session.refresh();
    assert_eq!(problems(&session, None).unwrap(), "no problems");

    // A `.lfy` file under the source directory that no earlier call saw is read in.
    // @lfy def/mcp/main.lfy:serve#serve:serve:9a771a2886ce7deadeb6a39ae4524d1bded9ac7c87fc49266ce3da78c7e4fbcc
    fixture.write("def/b.lfy", "const w = missing;\n");
    session.refresh();
    assert!(session.workspace.file("def/b.lfy").is_some());
    assert!(problems(&session, None).unwrap().contains("def/b.lfy:1:10"));

    // A file that has vanished is dropped the same way.
    // @lfy def/mcp/main.lfy:serve#serve:serve:01ce18f671ec478741555e6ff345d61120f118da6209c46d18cd01b3cbff3e6a
    fixture.remove("def/b.lfy");
    session.refresh();
    assert!(session.workspace.file("def/b.lfy").is_none());
    assert_eq!(problems(&session, None).unwrap(), "no problems");

    // An edit that keeps both the size and the modification time is not seen: the stamp
    // of the file is unchanged, so nothing is re-read.
    // @lfy def/mcp/main.lfy:serve#serve:serve:a0e0712c211c914b7d1655d1a85d5aa373db8b9d94524624d54554e2501f845d
    let path = fixture.root.join("def/a.lfy");
    let before = stamp(&path).unwrap();
    let text = fs::read_to_string(&path).unwrap().replace("const z = 1;", "const z = 2;");
    fs::write(&path, &text).unwrap();
    fs::File::open(&path).unwrap().set_modified(before.0.unwrap()).unwrap();
    assert_eq!(stamp(&path), Some(before));
    session.refresh();
    assert_eq!(session.seen.get("def/a.lfy"), Some(&before));

    // @lfy def/mcp/main.lfy:serve#serve:serve:4c5aeb9423c7e9206e75798da9c197b91b439325adda5776e09a2061d144988a
    fixture.write("elfie.json", r#"{ "name": "renamed", "source": "src", "lib": "lib" }"#);
    fixture.write("src/c.lfy", "const c = 1;\n");
    session.refresh();
    assert_eq!(session.workspace.name, "renamed");
    assert!(session.workspace.file("src/c.lfy").is_some());
    assert!(session.workspace.file("def/a.lfy").is_none());
}

/// Each session a host opens is given a number no earlier one had, is bound to what it
/// was opened with, reads its workspace at the binding's directory without re-reading a
/// file of the program, and is offered the tools of its role; closing it gives back what
/// `elfie_report` recorded in it, and a URL that is no open session's gives nothing.
// @lfy def/mcp/main.lfy:host#host:host:465078bd14c4c2971c8ae047a59db580f64b30e5fa762af2e656c4accbede7ba
// @lfy def/mcp/main.lfy:host#host:host:238a67527a5f81bdc05abb6515eacc7b86b9ead269c56d3b3654d34d47184b50
// @lfy def/mcp/main.lfy:host#host:host:b01f7a894faf3752cba01a91aaed7228df1bb1b93d87ea2c648f034427bcc3fd
// @lfy def/mcp/main.lfy:host#host:host:bb7c6b5258b2c4026015a8f9915717da1ec50aa40dcc10a43e5b50bc9e39310e
// @lfy def/mcp/main.lfy:host#host:host:dda4c7d26ad4016b54849285dc2862e998ac0d97cd234f0b56608729b03fdbb1
// @lfy def/mcp/main.lfy:host#host:host:7afa5703849ae259b65ae3bd7d32057445477122e6db4ac372c99154d461ea95
// @lfy def/mcp/main.lfy:host#host:host:91b44b23c29eaf7242b190da7fe5ab5f3642df5ec77212e680331af6b76e01b1
// @lfy def/mcp/main.lfy:host#host:host:64c0df55947a7e7a600bb0796c4715f740afcfda41cdd6f86b4cb408c4705713
// @lfy def/mcp/main.lfy:host#host:host:8eae0f57be7e371560c08488ce9a3c25a5eb3254d63aa6f4fcd5f317786e93b6
#[test]
fn a_hosted_session_is_bound_numbered_and_closed_with_what_it_recorded() {
    let fixture = compiled_project();
    let hosting = hosting_of("http://127.0.0.1:7/");
    // @lfy def/mcp/main.lfy:host#host:host:465078bd14c4c2971c8ae047a59db580f64b30e5fa762af2e656c4accbede7ba
    let first = hosting.open(binding_of(&fixture, Role::Compiler, Some("b+1")));
    assert_eq!(first, "http://127.0.0.1:7/sessions/1");
    let second = hosting.open(binding_of(&fixture, Role::Verifier, Some("b+1")));
    assert_eq!(second, "http://127.0.0.1:7/sessions/2");

    let compiling = hosting.session("/sessions/1").expect("the first session");
    let verifying = hosting.session("/sessions/2").expect("the second session");
    // A compiler is offered every tool carrying `tool` and not `commandTool` but
    // `report` and `reportMany`, the fns carrying `compilerTool` among them; a verifier
    // is offered every tool carrying `tool` alone but `check`. The fixture's target has
    // no command, so neither is offered a command tool.
    // @lfy def/mcp/main.lfy:host#host:host:dda4c7d26ad4016b54849285dc2862e998ac0d97cd234f0b56608729b03fdbb1
    assert_eq!(compiling.offered, COMPILER_TOOLS);
    // @lfy def/mcp/main.lfy:host#host:host:7afa5703849ae259b65ae3bd7d32057445477122e6db4ac372c99154d461ea95
    assert_eq!(verifying.offered, VERIFIER_TOOLS);
    assert!(verifying.offers("elfie_report"), "{:?}", verifying.offered);
    assert!(verifying.offers("elfie_reportMany"), "{:?}", verifying.offered);
    assert!(!verifying.offers("elfie_check"), "{:?}", verifying.offered);
    assert!(!compiling.offers("elfie_reportMany"), "{:?}", compiling.offered);
    // `Host.offered` gives the names of the tools the session at a URL is offered, in
    // the order the server lists them; a URL that is no open session's gives none.
    // @lfy def/mcp/main.lfy:host#host:host:d3b3abd60412e83fb1835597e514740cc7d4648f94a73575cf7f076399063025
    assert_eq!(hosting.offered(&first), COMPILER_TOOLS);
    assert_eq!(hosting.offered(&second), VERIFIER_TOOLS);
    // @lfy def/mcp/main.lfy:host#host:host:1f6860e467f9f7b17993f762e5ba4521688af80851fff707da5bbb18f86cdbba
    assert!(hosting.offered("http://127.0.0.1:7/sessions/9").is_empty());
    assert!(hosting.offered("http://elsewhere/sessions/1").is_empty());
    // `check` is not offered to it, so it is not listed either.
    // @lfy def/mcp/main.lfy:check#check:tool:d1b4319d34a3d00324ec0850bf1e0987ceb4112aab80f13d77b82554c27ca5f9
    assert!(!verifying.list().tools.iter().any(|tool| tool.name == "elfie_check"));
    // Each session's instructions name only the tools that session is offered, say which
    // of them change files, and say which role it is bound to. The fixture's target has
    // no command, so a verifier is offered nothing that changes a file.
    // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
    for (served, offered, withheld, role) in [
        (&compiling, COMPILER_TOOLS.as_slice(), "elfie_report", "compiler"),
        (&verifying, VERIFIER_TOOLS.as_slice(), "elfie_check", "verifier"),
    ] {
        let said = &served.instructions;
        for name in offered {
            assert!(said.contains(name), "{said}");
        }
        assert!(!said.contains(withheld), "{said}");
        assert!(said.contains(&format!("bound to a {role} run")), "{said}");
    }
    // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
    assert!(compiling.instructions.contains("elfie_mark writes a marker line"), "{}", compiling.instructions);
    assert!(verifying.instructions.contains("None of them changes a file."), "{}", verifying.instructions);
    {
        let session = compiling.session.lock().unwrap();
        let binding = session.binding.as_ref().expect("a binding");
        // @lfy def/mcp/main.lfy:host#host:host:238a67527a5f81bdc05abb6515eacc7b86b9ead269c56d3b3654d34d47184b50
        assert_eq!(binding.role, Role::Compiler);
        assert_eq!(binding.batch.as_ref().map(|batch| batch.identifier.as_str()), Some("b+1"));
        // @lfy def/mcp/main.lfy:host#host:host:b01f7a894faf3752cba01a91aaed7228df1bb1b93d87ea2c648f034427bcc3fd
        assert_eq!(session.workspace.root, fixture.root);
        assert_eq!(session.workspace.files.len(), binding.program.workspace.files.len());
        // No file of the program is stamped, so no call re-reads one.
        // @lfy def/mcp/main.lfy:host#host:host:bb7c6b5258b2c4026015a8f9915717da1ec50aa40dcc10a43e5b50bc9e39310e
        assert!(session.seen.is_empty());
        // @lfy def/mcp/main.lfy:host#host:host:91b44b23c29eaf7242b190da7fe5ab5f3642df5ec77212e680331af6b76e01b1
        assert!(session.reviews.lock().unwrap().is_empty());
        // @lfy def/mcp/main.lfy:host#host:host:5e5526603ec0ccf1f819bfc584b556e779b3a2a8f9d0f46fdc9b92ad91bb9dc5
        assert!(session.submission.lock().unwrap().is_none());
    }
    // A call naming a tool the session is not offered is an error saying so, and the fn
    // is not reached: nothing is checked and no plan is made.
    // @lfy def/mcp/main.lfy:check#check:tool:171e2f1ce748190f5ccbf577a5f91fc2fd8683a6b877bb654a16a48965e01265
    // @lfy def/mcp/main.lfy:check#check:tool:15fd0657dd9bfbe4565283f531743c14447968c8361b95ba4e874d2a8309c888
    let refused = verifying
        .call(&calling("elfie_check", serde_json::json!({ "unit": "b" })))
        .expect("a result");
    let (failed, text) = answered(&refused);
    assert!(failed, "{text}");
    assert_eq!(text, "elfie_check is not offered to this session");
    // A tool the session is offered is reached as ever.
    // @lfy def/mcp/main.lfy:units#units:tool:c0392f44087e7b29669f0f8d6b5d294cb0195675616839b4f6d80e791125f939
    let listed = compiling.call(&calling("elfie_units", serde_json::json!({}))).expect("a result");
    let (failed, text) = answered(&listed);
    assert!(!failed, "{text}");
    assert!(text.contains("rust b"), "{text}");

    // A compiler's session records what `elfie_submit` submitted, and closing it gives
    // that back with its reviews and leaves it no longer open.
    // @lfy def/mcp/main.lfy:host#host:host:64c0df55947a7e7a600bb0796c4715f740afcfda41cdd6f86b4cb408c4705713
    let submitted = compiling
        .call(&calling(
            "elfie_submit",
            serde_json::json!({ "outcome": "done", "message": "all four written" }),
        ))
        .expect("a result");
    let (failed, text) = answered(&submitted);
    assert!(!failed, "{text}");
    let record = hosting.close(&first);
    assert!(record.reviews.is_empty());
    assert_eq!(
        record.submission.as_ref().map(|one| (one.outcome, one.message.as_str())),
        Some((SubmittedOutcome::Done, "all four written"))
    );
    assert!(hosting.session("/sessions/1").is_none());
    // A URL that is no open session's gives a record holding nothing.
    // @lfy def/mcp/main.lfy:host#host:host:8eae0f57be7e371560c08488ce9a3c25a5eb3254d63aa6f4fcd5f317786e93b6
    for url in [first.as_str(), "http://127.0.0.1:7/sessions/9", "http://elsewhere/sessions/2"] {
        let record = hosting.close(url);
        assert!(record.reviews.is_empty() && record.submission.is_none(), "{url}");
    }
    assert!(hosting.session("/sessions/2").is_some());
}

/// One HTTP request to a URL on the loopback address, with a bearer token when one is
/// given: the status and the body.
fn http(url: &str, token: Option<&str>, body: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let rest = url.strip_prefix("http://").expect("an http url");
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    let mut stream = std::net::TcpStream::connect(authority).expect("the host listens");
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\
         Accept: application/json, text/event-stream\r\nContent-Type: application/json\r\n"
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let raw = String::from_utf8_lossy(&raw).into_owned();
    let status = raw
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("an HTTP answer, not {raw:?}"));
    let body = raw.split_once("\r\n\r\n").map(|(_, body)| body.to_string()).unwrap_or_default();
    (status, body)
}

/// The whole host, driven over HTTP: it listens on the loopback address at a port the
/// operating system picks, behind a token of its own; a request without the token is
/// unauthorized, one at a path no session is open at is not found, and one at a
/// session's URL speaks the protocol whatever MCP session id it carries. The port is no
/// longer listened on once `stop` returns.
// @lfy def/mcp/main.lfy:host#host:host:d17c6dc3479336c65543a2c99389c29582860e9358d65bd4e36a3d9c355e7825
// @lfy def/mcp/main.lfy:host#host:host:ea1cfd81c073494e7f7377472fc3070be780e4455e629f59e365d21d658269d4
// @lfy def/mcp/main.lfy:host#host:host:c0b5fd0495fb73ed8d21e6599998b9acc5a051a0dde254bb492f14f26417fade
// @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
// @lfy def/mcp/main.lfy:host#host:host:b20b250482898ccb1326689317f112cb544e9941949596a5c823ce56f4d692b9
// @lfy def/mcp/main.lfy:host#host:host:f876706edb4133d6c7e2e9d4f43bf7b7cc00c92572911e227d0c789a95636e5d
// @lfy def/mcp/main.lfy:host#host:host:c0519927a29b45c6d3ed0bf600d8dd929353a41c1678e7be62c950db70763d79
// @lfy def/mcp/main.lfy:host#host:host:e07851ea0cedfcd8e3d913fa15d72b1c389e0d5f87edfc1f52b7841decccbd1e
#[test]
fn the_host_speaks_the_protocol_over_http_behind_its_token() {
    /// What a call of `tools/list` reads.
    const LISTING: &str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let fixture = compiled_project();
    let host = host().expect("a host");
    // The loopback address, a colon, the port the operating system picked, and a slash.
    // @lfy def/mcp/main.lfy:host#host:host:ea1cfd81c073494e7f7377472fc3070be780e4455e629f59e365d21d658269d4
    // @lfy def/mcp/main.lfy:host#host:host:c0b5fd0495fb73ed8d21e6599998b9acc5a051a0dde254bb492f14f26417fade
    let address = host.url.strip_prefix("http://").and_then(|rest| rest.strip_suffix('/')).unwrap_or_default();
    let (name, port) = address.split_once(':').unwrap_or_else(|| panic!("{}", host.url));
    assert_eq!(name, LOOPBACK);
    assert_ne!(port.parse::<u16>().unwrap_or(0), 0, "{}", host.url);
    // 64 lowercase hex digits, new for each call of `host`.
    // @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
    assert_eq!(host.token.len(), TOKEN_BYTES * 2);
    assert!(
        host.token.chars().all(|digit| digit.is_ascii_hexdigit() && !digit.is_ascii_uppercase()),
        "{}",
        host.token
    );
    let other = crate::host().expect("a second host");
    assert_ne!(other.token, host.token);
    assert_ne!(other.url, host.url);
    (other.stop)();

    let url = (host.open)(binding_of(&fixture, Role::Compiler, Some("b+1")));
    // No token: 401, and nothing of the protocol is answered, so no tool ran.
    // @lfy def/mcp/main.lfy:host#host:host:b20b250482898ccb1326689317f112cb544e9941949596a5c823ce56f4d692b9
    let (status, body) = http(&url, None, LISTING);
    assert_eq!(status, 401, "{body}");
    assert!(!body.contains("jsonrpc"), "{body}");
    // The token, at a path no session is open at: 404, and no tool ran for it either.
    // @lfy def/mcp/main.lfy:host#host:host:f876706edb4133d6c7e2e9d4f43bf7b7cc00c92572911e227d0c789a95636e5d
    let (status, body) = http(&format!("{}sessions/9", host.url), Some(&host.token), LISTING);
    assert_eq!(status, 404, "{body}");
    assert!(!body.contains("jsonrpc"), "{body}");

    // The session's URL, under no MCP session id at all: the protocol is spoken, and
    // the listing is the session's.
    // @lfy def/mcp/main.lfy:host#host:host:d17c6dc3479336c65543a2c99389c29582860e9358d65bd4e36a3d9c355e7825
    // @lfy def/mcp/main.lfy:host#host:host:c0519927a29b45c6d3ed0bf600d8dd929353a41c1678e7be62c950db70763d79
    let (status, body) = http(&url, Some(&host.token), LISTING);
    assert_eq!(status, 200, "{body}");
    let listed: Value =
        serde_json::from_str(&body).unwrap_or_else(|error| panic!("a protocol answer, not {body:?}: {error}"));
    let served: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("a list of tools, not {listed}"))
        .iter()
        .map(|tool| tool["name"].as_str().expect("a tool name"))
        .collect();
    assert_eq!(served, COMPILER_TOOLS);
    // The same session answers at its URL whatever the request carries, so `Host.offered`
    // names the very tools it listed.
    // @lfy def/mcp/main.lfy:host#host:host:d3b3abd60412e83fb1835597e514740cc7d4648f94a73575cf7f076399063025
    assert_eq!((host.offered)(&url), COMPILER_TOOLS);
    // @lfy def/mcp/main.lfy:units#units:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    assert_eq!(listed["result"]["ttlMs"], serde_json::json!(0), "{listed}");
    assert_eq!(listed["result"]["cacheScope"], "private", "{listed}");

    // A call of a tool the session is offered answers with what the fn returns.
    // @lfy def/mcp/main.lfy:units#units:tool:59e3afe0a4f39564b82a347fa5494defa7421ae95a945c19049ed8e307b1b0e8
    let called = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"elfie_units","arguments":{}}}"#;
    let (status, body) = http(&url, Some(&host.token), called);
    assert_eq!(status, 200, "{body}");
    let called: Value = serde_json::from_str(&body).unwrap_or_else(|error| panic!("{body:?}: {error}"));
    let text = called["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("rust b"), "{called}");

    // Stop: every session is ended and the port is no longer listened on.
    // @lfy def/mcp/main.lfy:host#host:host:e07851ea0cedfcd8e3d913fa15d72b1c389e0d5f87edfc1f52b7841decccbd1e
    let address = address.to_string();
    (host.stop)();
    assert!(std::net::TcpStream::connect(&address).is_err(), "{address}");
}

/// The source maps a verifier's review was made with stand in for what the last
/// acceptance recorded, so the verifier reads the outputs it is judging.
// @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
#[test]
fn the_source_maps_of_a_review_stand_in_for_the_recorded_ones() {
    let fixture = accepted_project();
    let plain = fixture.session();
    let recorded = source_maps_of(&plain, None);
    let found = recorded
        .iter()
        .find(|map| map.output == "out/b.rs")
        .unwrap_or_else(|| panic!("a recorded map, not {recorded:?}"))
        .clone();
    // A map of the same target and the same source as a recorded one stands in for it.
    // @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
    let mut given = found.clone();
    given.output = "out/elsewhere.rs".to_string();
    let mut binding = binding_of(&fixture, Role::Verifier, None);
    binding.source_maps = vec![given];
    let session = bound(&fixture, binding);
    let read = source_maps_of(&session, None);
    assert!(read.iter().any(|map| map.output == "out/elsewhere.rs"), "{read:?}");
    assert!(!read.iter().any(|map| map.output == "out/b.rs"), "{read:?}");
    assert_eq!(read.len(), recorded.len());
    // With no map carried, what the last acceptance recorded is all that is read.
    // @lfy def/mcp/main.lfy:host#host:host:232c0725ae5219330822ed9664cc87be38c4591303cbc3dead664044e09b75eb
    let session = bound(&fixture, binding_of(&fixture, Role::Verifier, None));
    assert_eq!(source_maps_of(&session, None), recorded);
}

/// In a compiler's session the map of one of its own units is read from the markers its
/// output holds now, while every other member of it is the recorded map's, so the unit
/// still plans, changes, and requests as the compile planned it.
// @lfy def/mcp/main.lfy:host#host:host:7a6f6d82c62937a6200fb29971d7e37887f517b297d8c5a98583a469dfa4b7aa
#[test]
fn a_compilers_own_source_map_keeps_what_the_recorded_one_holds() {
    let fixture = Fixture::new();
    fixture.with_rust_target().write(
        "def/b.lfy",
        "fn b(): `A b` => string { @acceptanceCriteria.add({ behavior = `It answers` }); }\n",
    );
    let local = requirement_id(&fixture, "It answers");
    let accepted =
        format!("// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n");
    fixture.write("out/b.rs", &accepted);
    record_unit(&fixture, "b");
    let of_output = |maps: Vec<SourceMap>| {
        maps.into_iter()
            .find(|map| map.output == "out/b.rs")
            .unwrap_or_else(|| panic!("a map of out/b.rs"))
    };
    let recorded = of_output(source_maps_of(&fixture.session(), None));
    assert_eq!(recorded.markers.first().map(|marker| marker.output_line), Some(1));
    assert!(!recorded.hash.is_empty() && recorded.answered == vec![local.clone()], "{recorded:?}");
    // The output as it is now: a line was added before the marker, so its region begins
    // one line later than the recorded map says.
    fixture.write("out/b.rs", &format!("// A note.\n{accepted}"));
    // The unit is up to date, so the plan batches nothing; a compiler run of it is bound
    // to a batch of its own.
    let planned = plan_of(&fixture.session());
    let of_b = (0..planned.plan.units.len())
        .find(|&unit| planned.plan.units[unit].stem == "b")
        .expect("the unit b");
    let session = bound(
        &fixture,
        Binding {
            role: Role::Compiler,
            batch: Some(Batch { units: vec![of_b], identifier: "b".to_string() }),
            directory: fixture.root.clone(),
            plan: planned.plan.clone(),
            program: planned.program.clone(),
            request: None,
            review: None,
            source_maps: Vec::new(),
        },
    );
    let live = of_output(source_maps_of(&session, None));
    // @lfy def/mcp/main.lfy:host#host:host:7a6f6d82c62937a6200fb29971d7e37887f517b297d8c5a98583a469dfa4b7aa
    assert_eq!(live.markers.first().map(|marker| marker.output_line), Some(2));
    assert_ne!(live.markers, recorded.markers);
    // Nothing else differs: with the recorded markers put back, the two maps are one.
    // @lfy def/mcp/main.lfy:host#host:host:7a6f6d82c62937a6200fb29971d7e37887f517b297d8c5a98583a469dfa4b7aa
    let carried = SourceMap { markers: recorded.markers.clone(), ..live };
    assert_eq!(carried, recorded);
}

/// In a compiler's session a unit whose map file holds no map still has one map per
/// output file holding a marker of its own, read from the file as it is now, with every
/// other member of each empty, since nothing was ever recorded to carry over.
// @lfy def/mcp/main.lfy:host#host:host:8b0ee6f07bbcc53017d5574c5bc37d7cbf2fa810500d3cf286603559d1e4ecd5
#[test]
fn a_compilers_own_source_map_is_read_from_the_files_when_none_was_recorded() {
    let fixture = compiled_project();
    // Nothing has been accepted, so neither unit has a map; the output of `b` is written
    // and the output of `a` is not.
    fixture.write("out/b.rs", "// A note.\n// @lfy def/b.lfy:B\npub struct B;\n");
    let session = bound(&fixture, binding_of(&fixture, Role::Compiler, Some("b+1")));
    let maps = source_maps_of(&session, None);
    // @lfy def/mcp/main.lfy:host#host:host:8b0ee6f07bbcc53017d5574c5bc37d7cbf2fa810500d3cf286603559d1e4ecd5
    let read: Vec<&SourceMap> = maps.iter().filter(|map| map.source == "def/b.lfy").collect();
    let [map] = read[..] else { panic!("one map of def/b.lfy, not {read:?}") };
    assert_eq!((map.target.as_str(), map.output.as_str()), ("rust", "out/b.rs"));
    // The markers are the ones the file holds now, read as acceptance reads them.
    // @lfy def/mcp/main.lfy:host#host:host:8b0ee6f07bbcc53017d5574c5bc37d7cbf2fa810500d3cf286603559d1e4ecd5
    assert_eq!(map.markers.len(), 1);
    assert_eq!(map.markers[0].output_line, 2);
    assert_eq!(map.markers[0].entity.as_deref(), Some("B"));
    // Every other member is empty.
    // @lfy def/mcp/main.lfy:host#host:host:8b0ee6f07bbcc53017d5574c5bc37d7cbf2fa810500d3cf286603559d1e4ecd5
    let empty = SourceMap {
        markers: map.markers.clone(),
        target: map.target.clone(),
        output: map.output.clone(),
        source: map.source.clone(),
        hash: String::new(),
        requirements: String::new(),
        answered: Vec::new(),
        signature: String::new(),
        text: String::new(),
        dependencies: BTreeMap::new(),
        generated: String::new(),
    };
    assert_eq!(map, &empty);
    // No file holds a marker of `a`, so it has no map at all.
    // @lfy def/mcp/main.lfy:host#host:host:8b0ee6f07bbcc53017d5574c5bc37d7cbf2fa810500d3cf286603559d1e4ecd5
    assert!(!maps.iter().any(|map| map.source == "def/a.lfy"), "{maps:?}");
}

/// A hosted session acts on its own batch and on no other: a request, a check, a
/// change, or a review for another is an error naming the batch it acts on, or the
/// global review when it is bound to none.
// @lfy def/mcp/main.lfy:request#request:request:f7ef0e526d107bc01f393317930dc6691e83904a9ec731aef3e304884867cfe0
// @lfy def/mcp/main.lfy:check#check:check:9c073d6b09e272ea0d5a712167c00378477b35e103dad16445f0d53b88d7bc30
// @lfy def/mcp/main.lfy:changes#changes:changes:e97791dbc253b4037be4b5a6a54aa430027a02f99710070c689449608a3affe1
// @lfy def/mcp/main.lfy:review#review:review:0dcb77dad8ef4db5102c1bd7eeaec630f239dc244db5eee8132f8c57d77677ea
#[test]
fn a_hosted_session_acts_only_on_its_own_batch() {
    let fixture = compiled_project();
    let plain = fixture.session();
    let planned = plan_of(&plain);
    // The fixture batches both units as `b+1`; a binding to a batch of its own holding
    // only `a` leaves the unit `b` and the batch `b+1` outside it.
    let of_a = (0..planned.plan.units.len())
        .find(|&unit| planned.plan.units[unit].stem == "a")
        .expect("the unit a");
    let mut binding = binding_of(&fixture, Role::Compiler, Some("b+1"));
    binding.batch = Some(Batch {
        units: vec![of_a],
        identifier: "a".to_string(),
    });
    let session = bound(&fixture, binding);
    // @lfy def/mcp/main.lfy:request#request:request:f7ef0e526d107bc01f393317930dc6691e83904a9ec731aef3e304884867cfe0
    let error = request(&session, "b+1", None).unwrap_err();
    assert!(error.contains("acts only on its own batch, a"), "{error}");
    // @lfy def/mcp/main.lfy:check#check:check:9c073d6b09e272ea0d5a712167c00378477b35e103dad16445f0d53b88d7bc30
    let error = check(&session, "b", None).unwrap_err();
    assert!(error.contains("acts only on its own batch, a"), "{error}");
    // @lfy def/mcp/main.lfy:changes#changes:changes:e97791dbc253b4037be4b5a6a54aa430027a02f99710070c689449608a3affe1
    let error = changes(&session, "b", None).unwrap_err();
    assert!(error.contains("acts only on its own batch, a"), "{error}");
    // @lfy def/mcp/main.lfy:review#review:review:0dcb77dad8ef4db5102c1bd7eeaec630f239dc244db5eee8132f8c57d77677ea
    let error = review(&session, "b+1", None).unwrap_err();
    assert!(error.contains("acts only on its own batch, a"), "{error}");
    // Its own unit is answered for as ever: the guard lets it through and the verdict
    // of `accept` is what comes back.
    // @lfy def/mcp/main.lfy:check#check:check:2a16ea176547468dbe87eb74de7c195dc58e16c26122fc69de5f2ff15dd469f1
    let error = check(&session, "a", None).unwrap_err();
    assert!(error.starts_with("rejected a:"), "{error}");

    // A session bound to the global review acts on no batch at all.
    // @lfy def/mcp/main.lfy:review#review:review:0dcb77dad8ef4db5102c1bd7eeaec630f239dc244db5eee8132f8c57d77677ea
    let session = bound(&fixture, binding_of(&fixture, Role::Verifier, None));
    let error = review(&session, "b+1", None).unwrap_err();
    assert!(error.contains("acts only on the global review"), "{error}");
    let error = request(&session, "b+1", None).unwrap_err();
    assert!(error.contains("acts only on the global review"), "{error}");
    let error = changes(&session, "b", None).unwrap_err();
    assert!(error.contains("acts only on the global review"), "{error}");
}

/// A verifier is given the review request it was run with, never one made again from
/// the outputs as they are now; a compiler of the same batch has the request made for
/// the batch.
// @lfy def/mcp/main.lfy:review#review:review:da108e476bfaee29f25ace2bffd41fea2a2d7605e41824c4b25757bf8287f9b1
#[test]
fn a_verifier_is_given_the_review_request_it_was_run_with() {
    /// What the review request the verifier was run with reads.
    const GIVEN: &str = "# Reviewing the batch `b`\n\nthe request this verifier was run with\n";
    let fixture = project_with_requirements();
    let mut binding = binding_of(&fixture, Role::Verifier, Some("b"));
    let mut given = binding.review.clone().expect("a verifier's review");
    given.instructions = GIVEN.to_string();
    binding.review = Some(given);
    let session = bound(&fixture, binding);
    // @lfy def/mcp/main.lfy:review#review:review:da108e476bfaee29f25ace2bffd41fea2a2d7605e41824c4b25757bf8287f9b1
    assert_eq!(review(&session, "b", None).unwrap(), GIVEN);
    // A compiler's session has the request made for the batch instead.
    // @lfy def/mcp/main.lfy:review#review:review:f58d35f9aea99f8ea42d7e215a8e84d42b2b414afa745217d99622b1a66e5c0d
    let session = bound(&fixture, binding_of(&fixture, Role::Compiler, Some("b")));
    let instructions = review(&session, "b", None).unwrap();
    assert_ne!(instructions, GIVEN);
    assert!(instructions.contains("It answers"), "{instructions}");
}
